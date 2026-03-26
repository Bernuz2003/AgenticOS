use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::WorkloadClass;
use crate::orchestrator::Orchestrator;
use crate::process::{ProcessLifecyclePolicy, ProcessState};
use crate::scheduler::ProcessPriority;
use crate::scheduler::ProcessScheduler;
use crate::services::process_runtime::{
    kill_managed_process_with_session, spawn_managed_process_with_session, ManagedProcessRequest,
};
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::tools::human_tools::{normalize_ask_human_request, AskHumanInput};
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};
use crate::tools::{handle_syscall, SysCallOutcome, SyscallRateMap};
use crate::{audit, audit::AuditContext};
use agentic_control_models::KernelEvent;
use mio::Waker;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use super::actions::{self, ActionInvocation, ActionName};

#[derive(Debug)]
pub(crate) enum SyscallCmd {
    Execute {
        pid: u64,
        tool_call_id: String,
        content: String,
        caller: ToolCaller,
        permissions: ProcessPermissionPolicy,
        registry: ToolRegistry,
    },
    Shutdown,
}

#[derive(Debug)]
pub(crate) struct SyscallCompletion {
    pub pid: u64,
    pub tool_call_id: String,
    pub command: String,
    pub caller: ToolCaller,
    pub outcome: SysCallOutcome,
}

static TOOL_CALL_COUNTER: AtomicU64 = AtomicU64::new(1);
static IPC_MESSAGE_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_tool_call_id(pid: u64) -> String {
    let seq = TOOL_CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("tool-{pid}-{seq}")
}

fn next_ipc_message_id() -> String {
    let seq = IPC_MESSAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("ipc-{}-{seq}", crate::storage::current_timestamp_ms())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SyscallDispatchOutcome {
    None,
    Queued,
    Spawned(u64),
    Killed,
}

pub(crate) fn spawn_syscall_worker(
    rate_map: Arc<Mutex<SyscallRateMap>>,
    result_tx: mpsc::Sender<SyscallCompletion>,
    cmd_rx: mpsc::Receiver<SyscallCmd>,
    wake_loop: Option<Arc<Waker>>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("syscall-worker".into())
        .spawn(move || {
            while let Ok(command) = cmd_rx.recv() {
                match command {
                    SyscallCmd::Execute {
                        pid,
                        tool_call_id,
                        content,
                        caller,
                        permissions,
                        registry,
                    } => {
                        let command = content.clone();
                        let outcome = match rate_map.lock() {
                            Ok(mut guard) => handle_syscall(
                                &content,
                                pid,
                                caller.clone(),
                                permissions,
                                Some(tool_call_id.clone()),
                                &mut guard,
                                &registry,
                            ),
                            Err(_) => SysCallOutcome {
                                output: "SysCall Error: worker rate-limit state is unavailable."
                                    .to_string(),
                                success: false,
                                duration_ms: 0,
                                should_kill_process: true,
                            },
                        };
                        if result_tx
                            .send(SyscallCompletion {
                                pid,
                                tool_call_id,
                                command,
                                caller,
                                outcome,
                            })
                            .is_err()
                        {
                            break;
                        }
                        if let Some(waker) = wake_loop.as_ref() {
                            let _ = waker.wake();
                        }
                    }
                    SyscallCmd::Shutdown => break,
                }
            }
        })
        .expect("failed to spawn syscall worker")
}

fn extract_tool_name(content: &str) -> Option<&str> {
    let trimmed = content.trim();
    let rest = trimmed.strip_prefix("TOOL:")?;
    let end = rest
        .find(|ch: char| ch.is_whitespace() || ch == '{')
        .unwrap_or(rest.len());
    let name = &rest[..end];
    (!name.is_empty()).then_some(name)
}

fn likely_superfluous_tool_call(content: &str) -> Option<&'static str> {
    let trimmed = content.trim();
    if trimmed.starts_with("TOOL:python")
        && (trimmed.contains("\"print(")
            || trimmed.contains("\"code\":\"echo")
            || trimmed.contains("\"code\":\"format"))
    {
        return Some("python_text_only");
    }
    if trimmed.starts_with("TOOL:calc")
        && (trimmed.contains("1+1")
            || trimmed.contains("2+2")
            || trimmed.contains("3+3")
            || trimmed.contains("4+4"))
    {
        return Some("calc_trivial_expression");
    }
    None
}

pub(super) fn scan_syscall_buffer(buffer: &mut String) -> Option<String> {
    let mut absolute_offset = 0usize;
    for line in buffer.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let leading_ws = line.len() - trimmed.len();
        let marker_offset = absolute_offset + leading_ws;

        for prefix in ["ACTION:", "TOOL:"] {
            if !trimmed.starts_with(prefix) {
                continue;
            }

            let candidate = &buffer[marker_offset..];
            match crate::text_invocation::extract_prefixed_json_invocation(candidate, prefix) {
                crate::text_invocation::PrefixedInvocationExtract::Parsed(parsed) => {
                    let consumed = marker_offset + parsed.consumed_bytes;
                    let command = parsed.raw_invocation;
                    buffer.drain(..consumed);
                    return Some(command);
                }
                crate::text_invocation::PrefixedInvocationExtract::Incomplete => return None,
                crate::text_invocation::PrefixedInvocationExtract::Invalid(_) => {
                    let newline_rel = candidate.find('\n');
                    let command_end = newline_rel.unwrap_or(candidate.len());
                    let command = candidate[..command_end].trim_end_matches('\r').to_string();
                    let consumed = marker_offset + newline_rel.map_or(command_end, |idx| idx + 1);
                    buffer.drain(..consumed);
                    return Some(command);
                }
            }
        }

        absolute_offset += line.len();
    }

    if buffer.len() > 8000 {
        buffer.clear();
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_process_syscall(
    runtime_id: &str,
    pid_floor: u64,
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    orchestrator: &Orchestrator,
    pid: u64,
    content: &str,
    syscall_cmd_tx: &mpsc::Sender<SyscallCmd>,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
    tool_registry: &ToolRegistry,
) -> SyscallDispatchOutcome {
    let caller = engine
        .processes
        .get(&pid)
        .map(|process| process.tool_caller.clone())
        .unwrap_or(ToolCaller::AgentText);
    let permissions = engine
        .processes
        .get(&pid)
        .map(|process| process.permission_policy.clone())
        .unwrap_or_else(|| ProcessPermissionPolicy {
            trust_scope: crate::tools::invocation::ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: Vec::new(),
            path_scopes: vec![".".to_string()],
        });

    let quota_exceeded = scheduler.record_syscall(pid);
    if quota_exceeded {
        tracing::warn!(pid, "SCHEDULER: syscall quota exceeded — killing process");
        kill_managed_process_with_session(
            engine,
            memory,
            scheduler,
            session_registry,
            storage,
            pid,
            "syscall_quota_exceeded",
        );
        return SyscallDispatchOutcome::Killed;
    }

    if content.starts_with("ACTION:") {
        if !caller.can_orchestrate_actions() || !permissions.actions_allowed {
            audit::record(
                storage,
                audit::ACTION_DENIED,
                format!(
                    "command={} caller={} trust_scope={} reason=policy_actions_disabled",
                    content,
                    caller.as_str(),
                    permissions.trust_scope
                ),
                AuditContext::for_process(
                    session_registry.session_id_for_pid(pid),
                    pid,
                    Some(runtime_id),
                ),
            );
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(
                    "ACTION Error: this session is interactive and cannot orchestrate other agents.",
                ),
            );
            return SyscallDispatchOutcome::None;
        }

        match actions::parse_text_invocation(content) {
            Ok(invocation) => {
                audit::record(
                    storage,
                    audit::ACTION_DISPATCHED,
                    format!(
                        "command={} caller={} trust_scope={}",
                        content,
                        caller.as_str(),
                        permissions.trust_scope
                    ),
                    AuditContext::for_process(
                        session_registry.session_id_for_pid(pid),
                        pid,
                        Some(runtime_id),
                    ),
                );
                dispatch_action_invocation(
                    runtime_id,
                    pid_floor,
                    engine,
                    memory,
                    scheduler,
                    orchestrator,
                    pid,
                    invocation,
                    session_registry,
                    storage,
                    pending_events,
                    tool_registry,
                )
            }
            Err(err) => {
                audit::record(
                    storage,
                    audit::ACTION_DENIED,
                    format!(
                        "command={} caller={} trust_scope={} reason=parse_error detail={}",
                        content,
                        caller.as_str(),
                        permissions.trust_scope,
                        err
                    ),
                    AuditContext::for_process(
                        session_registry.session_id_for_pid(pid),
                        pid,
                        Some(runtime_id),
                    ),
                );
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(&format!("ACTION Error: {}", err)),
                );
                SyscallDispatchOutcome::None
            }
        }
    } else {
        let tool_call_id = next_tool_call_id(pid);
        if let Some(outcome) = dispatch_native_human_input_request(
            runtime_id,
            engine,
            pid,
            content,
            &tool_call_id,
            &caller,
            &permissions,
            session_registry,
            storage,
            pending_events,
        ) {
            return outcome;
        }
        audit::record(
            storage,
            audit::TOOL_DISPATCHED,
            format!(
                "tool_call_id={} command={} caller={} transport=text",
                tool_call_id,
                content,
                caller.as_str()
            ),
            AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(runtime_id),
            ),
        );
        if let Some(reason) = likely_superfluous_tool_call(content) {
            audit::record(
                storage,
                audit::TOOL_USAGE_DIAGNOSTIC,
                format!(
                    "tool_call_id={} reason={} tool_name={} command={}",
                    tool_call_id,
                    reason,
                    extract_tool_name(content).unwrap_or("unknown"),
                    content
                ),
                AuditContext::for_process(
                    session_registry.session_id_for_pid(pid),
                    pid,
                    Some(runtime_id),
                ),
            );
        }
        let queued = syscall_cmd_tx.send(SyscallCmd::Execute {
            pid,
            tool_call_id: tool_call_id.clone(),
            content: content.to_string(),
            caller,
            permissions,
            registry: tool_registry.clone(),
        });
        match queued {
            Ok(()) => {
                if let Some(process) = engine.processes.get_mut(&pid) {
                    process.state = ProcessState::WaitingForSyscall;
                }
                pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "syscall_dispatched".to_string(),
                });
                SyscallDispatchOutcome::Queued
            }
            Err(err) => {
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(&format!(
                        "SysCall Error: failed to enqueue execution: {}",
                        err
                    )),
                );
                if let Some(process) = engine.processes.get_mut(&pid) {
                    process.state = ProcessState::Ready;
                }
                SyscallDispatchOutcome::None
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_native_human_input_request(
    runtime_id: &str,
    engine: &mut LLMEngine,
    pid: u64,
    content: &str,
    tool_call_id: &str,
    caller: &ToolCaller,
    permissions: &ProcessPermissionPolicy,
    session_registry: &SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
) -> Option<SyscallDispatchOutcome> {
    let invocation = crate::tools::parser::parse_text_invocation(content).ok()?;
    if invocation.name != "ask_human" {
        return None;
    }

    let audit_context = AuditContext::for_process(
        session_registry.session_id_for_pid(pid),
        pid,
        Some(runtime_id),
    );
    audit::record(
        storage,
        audit::TOOL_DISPATCHED,
        format!(
            "tool_call_id={} command={} caller={} transport=text native=true",
            tool_call_id,
            content,
            caller.as_str()
        ),
        audit_context.clone(),
    );

    if !permissions.allows_tool("ask_human") {
        let _ = engine.inject_context(
            pid,
            &engine.format_system_message(
                "TOOL Error: ask_human is not allowed for this process policy.",
            ),
        );
        if let Some(process) = engine.processes.get_mut(&pid) {
            process.state = ProcessState::Ready;
        }
        audit::record(
            storage,
            audit::TOOL_FAILED,
            format!(
                "tool_call_id={} command={} caller={} transport=text detail=policy_denied",
                tool_call_id,
                content,
                caller.as_str()
            ),
            audit_context,
        );
        return Some(SyscallDispatchOutcome::None);
    }

    let ask_human_input = match serde_json::from_value::<AskHumanInput>(invocation.input.clone()) {
        Ok(input) => input,
        Err(err) => {
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(&format!(
                    "TOOL Error: invalid ask_human payload: {err}",
                )),
            );
            if let Some(process) = engine.processes.get_mut(&pid) {
                process.state = ProcessState::Ready;
            }
            audit::record(
                storage,
                audit::TOOL_FAILED,
                format!(
                    "tool_call_id={} command={} caller={} transport=text detail=invalid_payload:{}",
                    tool_call_id,
                    content,
                    caller.as_str(),
                    err
                ),
                audit_context,
            );
            return Some(SyscallDispatchOutcome::None);
        }
    };

    let request = match normalize_ask_human_request(ask_human_input) {
        Ok(request) => request,
        Err(err) => {
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(&format!("TOOL Error: {}", err)),
            );
            if let Some(process) = engine.processes.get_mut(&pid) {
                process.state = ProcessState::Ready;
            }
            audit::record(
                storage,
                audit::TOOL_FAILED,
                format!(
                    "tool_call_id={} command={} caller={} transport=text detail={}",
                    tool_call_id,
                    content,
                    caller.as_str(),
                    err
                ),
                audit_context,
            );
            return Some(SyscallDispatchOutcome::None);
        }
    };

    let question = request.question.clone();
    let kind = request.kind.as_str().to_string();
    let choices = request.choices.join("|");
    if let Some(process) = engine.processes.get_mut(&pid) {
        process.set_pending_human_request(request);
        process.state = ProcessState::WaitingForHumanInput;
    }
    pending_events.push(KernelEvent::WorkspaceChanged {
        pid,
        reason: "human_input_requested".to_string(),
    });
    pending_events.push(KernelEvent::LobbyChanged {
        reason: "human_input_requested".to_string(),
    });
    audit::record(
        storage,
        audit::PROCESS_HUMAN_INPUT_REQUESTED,
        format!(
            "tool_call_id={} kind={} question={} choices={}",
            tool_call_id, kind, question, choices
        ),
        audit_context.clone(),
    );
    audit::record(
        storage,
        audit::TOOL_COMPLETED,
        format!(
            "tool_call_id={} command={} caller={} transport=text detail=pending_human_input",
            tool_call_id,
            content,
            caller.as_str()
        ),
        audit_context,
    );
    Some(SyscallDispatchOutcome::None)
}

fn non_empty_input_str<'a>(input: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn build_ipc_payload_preview(payload_text: &str) -> String {
    if payload_text.chars().count() > 240 {
        let mut preview = payload_text.chars().take(240).collect::<String>();
        preview.push_str("...");
        preview
    } else {
        payload_text.to_string()
    }
}

fn task_role_for_binding(
    orchestrator: &Orchestrator,
    binding: &crate::orchestrator::TaskPidBinding,
) -> Option<String> {
    orchestrator
        .get(binding.orch_id)
        .and_then(|orch| orch.tasks.get(&binding.task_id))
        .and_then(|task| task.role.clone())
        .map(|role| role.trim().to_string())
        .filter(|role| !role.is_empty())
}

fn resolve_named_task_target(
    orchestrator: &Orchestrator,
    orch_id: u64,
    task_id: &str,
) -> Option<(Option<u64>, Option<u32>, Option<String>)> {
    let orch = orchestrator.get(orch_id)?;
    let task = orch.tasks.get(task_id)?;
    let role = task
        .role
        .clone()
        .map(|role| role.trim().to_string())
        .filter(|role| !role.is_empty());
    let attempt = match orch.status.get(task_id) {
        Some(crate::orchestrator::TaskStatus::Running { pid, attempt }) => {
            return Some((Some(*pid), Some(*attempt), role));
        }
        Some(crate::orchestrator::TaskStatus::Completed { attempt })
        | Some(crate::orchestrator::TaskStatus::Failed { attempt, .. }) => Some(*attempt),
        Some(
            crate::orchestrator::TaskStatus::Pending | crate::orchestrator::TaskStatus::Skipped,
        ) => None,
        None => return None,
    };
    Some((None, attempt, role))
}

fn resolve_role_target(
    orchestrator: &Orchestrator,
    orch_id: u64,
    role: &str,
) -> Option<(String, Option<u64>, Option<u32>)> {
    let orch = orchestrator.get(orch_id)?;
    let target_role = role.trim();
    if target_role.is_empty() {
        return None;
    }

    let mut fallback: Option<(String, Option<u64>, Option<u32>)> = None;
    for task_id in &orch.topo_order {
        let Some(task) = orch.tasks.get(task_id) else {
            continue;
        };
        let matches_role = task
            .role
            .as_deref()
            .map(str::trim)
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(target_role));
        if !matches_role {
            continue;
        }

        match orch.status.get(task_id) {
            Some(crate::orchestrator::TaskStatus::Running { pid, attempt }) => {
                return Some((task_id.clone(), Some(*pid), Some(*attempt)));
            }
            Some(crate::orchestrator::TaskStatus::Completed { attempt })
            | Some(crate::orchestrator::TaskStatus::Failed { attempt, .. }) => {
                fallback.get_or_insert((task_id.clone(), None, Some(*attempt)));
            }
            Some(
                crate::orchestrator::TaskStatus::Pending | crate::orchestrator::TaskStatus::Skipped,
            ) => {
                fallback.get_or_insert((task_id.clone(), None, None));
            }
            None => {}
        }
    }

    fallback
}

fn format_ipc_sender_label(message: &crate::storage::StoredIpcMessage) -> String {
    message
        .sender_task_id
        .clone()
        .or_else(|| message.sender_pid.map(|pid| format!("pid {pid}")))
        .unwrap_or_else(|| "unknown sender".to_string())
}

fn format_ipc_receiver_label(message: &crate::storage::StoredIpcMessage) -> String {
    message
        .receiver_task_id
        .clone()
        .or_else(|| {
            message
                .receiver_role
                .clone()
                .map(|role| format!("role:{role}"))
        })
        .or_else(|| {
            message
                .channel
                .clone()
                .map(|channel| format!("channel:{channel}"))
        })
        .or_else(|| message.receiver_pid.map(|pid| format!("pid {pid}")))
        .unwrap_or_else(|| "unknown target".to_string())
}

fn channel_mailbox_match(
    selector: &crate::storage::IpcMailboxSelector,
    message: &crate::storage::StoredIpcMessage,
) -> bool {
    selector.orchestration_id.is_some()
        && message.orchestration_id == selector.orchestration_id
        && message.channel.is_some()
        && message.receiver_pid.is_none()
        && message.receiver_task_id.is_none()
        && message.receiver_role.is_none()
}

#[allow(clippy::too_many_arguments)]
fn dispatch_action_invocation(
    runtime_id: &str,
    pid_floor: u64,
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    orchestrator: &Orchestrator,
    pid: u64,
    invocation: ActionInvocation,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
    tool_registry: &ToolRegistry,
) -> SyscallDispatchOutcome {
    match invocation.action {
        ActionName::Spawn => {
            let Some(prompt) = invocation
                .input
                .get("prompt")
                .and_then(|value| value.as_str())
            else {
                audit::record(
                    storage,
                    audit::ACTION_DENIED,
                    "action=spawn reason=missing_prompt",
                    AuditContext::for_process(
                        session_registry.session_id_for_pid(pid),
                        pid,
                        Some(runtime_id),
                    ),
                );
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(
                        "ACTION Error: ACTION:spawn requires string field 'prompt'.",
                    ),
                );
                return SyscallDispatchOutcome::None;
            };
            dispatch_spawn_action(
                runtime_id,
                pid_floor,
                engine,
                memory,
                scheduler,
                pid,
                prompt,
                session_registry,
                storage,
                pending_events,
                tool_registry,
            )
        }
        ActionName::Send => {
            let target_pid = invocation.input.get("pid").and_then(|value| value.as_u64());
            let target_task = non_empty_input_str(&invocation.input, "task");
            let target_role = non_empty_input_str(&invocation.input, "role");
            let message = invocation
                .input
                .get("message")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let payload = invocation.input.get("payload").cloned();
            let message_type = invocation
                .input
                .get("message_type")
                .and_then(|value| value.as_str())
                .unwrap_or("notification");
            let channel = non_empty_input_str(&invocation.input, "channel");
            let explicit_orchestration_id = invocation
                .input
                .get("orchestration_id")
                .and_then(|value| value.as_u64());

            if target_pid.is_none()
                && target_task.is_none()
                && target_role.is_none()
                && channel.is_none()
            {
                audit::record(
                    storage,
                    audit::ACTION_DENIED,
                    "action=send reason=missing_target",
                    AuditContext::for_process(
                        session_registry.session_id_for_pid(pid),
                        pid,
                        Some(runtime_id),
                    ),
                );
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(
                        "ACTION Error: ACTION:send requires a target: pid, task, role or channel.",
                    ),
                );
                return SyscallDispatchOutcome::None;
            }
            if message.is_none() && payload.is_none() {
                audit::record(
                    storage,
                    audit::ACTION_DENIED,
                    "action=send reason=missing_payload",
                    AuditContext::for_process(
                        session_registry.session_id_for_pid(pid),
                        pid,
                        Some(runtime_id),
                    ),
                );
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(
                        "ACTION Error: ACTION:send requires 'message' or 'payload'.",
                    ),
                );
                return SyscallDispatchOutcome::None;
            }
            dispatch_send_action(
                runtime_id,
                engine,
                orchestrator,
                pid,
                target_pid,
                target_task,
                target_role,
                explicit_orchestration_id,
                message.as_deref(),
                payload.as_ref(),
                message_type,
                channel,
                session_registry,
                storage,
            );
            SyscallDispatchOutcome::None
        }
        ActionName::Receive => {
            let limit = invocation
                .input
                .get("limit")
                .and_then(|value| value.as_u64())
                .unwrap_or(4)
                .clamp(1, 16) as usize;
            let include_delivered = invocation
                .input
                .get("include_delivered")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let channel = non_empty_input_str(&invocation.input, "channel");
            dispatch_receive_action(
                runtime_id,
                engine,
                orchestrator,
                pid,
                limit,
                channel,
                include_delivered,
                session_registry,
                storage,
            );
            SyscallDispatchOutcome::None
        }
        ActionName::Ack => {
            let message_ids = invocation
                .input
                .get("message_ids")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|value| value.as_str())
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if message_ids.is_empty() {
                audit::record(
                    storage,
                    audit::ACTION_DENIED,
                    "action=ack reason=missing_message_ids",
                    AuditContext::for_process(
                        session_registry.session_id_for_pid(pid),
                        pid,
                        Some(runtime_id),
                    ),
                );
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(
                        "ACTION Error: ACTION:ack requires a non-empty 'message_ids' array.",
                    ),
                );
                return SyscallDispatchOutcome::None;
            }
            dispatch_ack_action(
                runtime_id,
                engine,
                orchestrator,
                pid,
                &message_ids,
                session_registry,
                storage,
            );
            SyscallDispatchOutcome::None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_spawn_action(
    runtime_id: &str,
    pid_floor: u64,
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    pid: u64,
    prompt: &str,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
    tool_registry: &ToolRegistry,
) -> SyscallDispatchOutcome {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        audit::record(
            storage,
            audit::ACTION_DENIED,
            "action=spawn reason=empty_prompt",
            AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(runtime_id),
            ),
        );
        let _ = engine.inject_context(
            pid,
            &engine
                .format_system_message("ACTION Error: ACTION:spawn requires a non-empty 'prompt'."),
        );
        return SyscallDispatchOutcome::None;
    }

    let owner_id = engine.process_owner_id(pid).unwrap_or(0);
    let parent_sched = scheduler.snapshot(pid);
    let workload = parent_sched
        .as_ref()
        .map(|snapshot| snapshot.workload)
        .unwrap_or(WorkloadClass::General);
    let priority = parent_sched
        .as_ref()
        .map(|snapshot| snapshot.priority)
        .unwrap_or(ProcessPriority::Normal);
    let inherited_context_policy = engine
        .processes
        .get(&pid)
        .map(|process| process.context_policy.clone());
    let child_permission_policy = engine
        .processes
        .get(&pid)
        .map(|process| process.permission_policy.derive_chat_child())
        .or_else(|| {
            crate::tools::invocation::ProcessPermissionPolicy::interactive_chat(tool_registry).ok()
        });

    if runtime_id.is_empty() {
        audit::record(
            storage,
            audit::ACTION_DENIED,
            "action=spawn reason=missing_runtime_binding",
            AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(runtime_id),
            ),
        );
        let _ = engine.inject_context(
            pid,
            &engine.format_system_message("ERROR: Runtime binding not found for SPAWN syscall."),
        );
        return SyscallDispatchOutcome::None;
    }

    match spawn_managed_process_with_session(
        runtime_id,
        pid_floor,
        engine,
        memory,
        scheduler,
        session_registry,
        storage,
        ManagedProcessRequest {
            prompt: prompt.to_string(),
            system_prompt: Some(crate::agent_prompt::build_agent_system_prompt(
                tool_registry,
                ToolCaller::AgentText,
            )),
            owner_id,
            tool_caller: ToolCaller::AgentText,
            permission_policy: child_permission_policy,
            workload,
            required_backend_class: None,
            priority,
            lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
            context_policy: inherited_context_policy,
        },
    ) {
        Ok(new_pid) => {
            audit::record(
                storage,
                audit::ACTION_COMPLETED,
                format!(
                    "action=spawn child_pid={} workload={:?}",
                    new_pid.pid, workload
                ),
                AuditContext::for_process(
                    session_registry.session_id_for_pid(pid),
                    pid,
                    Some(runtime_id),
                ),
            );
            pending_events.push(KernelEvent::SessionStarted {
                session_id: new_pid.session_id.clone(),
                pid: new_pid.pid,
                workload: format!("{:?}", workload).to_lowercase(),
                prompt: prompt.to_string(),
            });
            pending_events.push(KernelEvent::WorkspaceChanged {
                pid: new_pid.pid,
                reason: "syscall_spawned".to_string(),
            });
            pending_events.push(KernelEvent::LobbyChanged {
                reason: "syscall_spawned".to_string(),
            });
            let msg = format!("SUCCESS: Worker Created (PID {}).", new_pid.pid);
            let feedback = engine.format_system_message(&msg);
            let _ = engine.inject_context(pid, &feedback);
            SyscallDispatchOutcome::Spawned(new_pid.pid)
        }
        Err(e) => {
            audit::record(
                storage,
                audit::ACTION_DENIED,
                format!("action=spawn reason=spawn_failed detail={}", e),
                AuditContext::for_process(
                    session_registry.session_id_for_pid(pid),
                    pid,
                    Some(runtime_id),
                ),
            );
            let _ =
                engine.inject_context(pid, &engine.format_system_message(&format!("ERROR: {}", e)));
            SyscallDispatchOutcome::None
        }
    }
}

pub(super) fn drain_syscall_results(
    runtime_registry: &mut crate::runtimes::RuntimeRegistry,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    result_rx: &mpsc::Receiver<SyscallCompletion>,
    pending_events: &mut Vec<KernelEvent>,
) -> usize {
    let mut processed_results = 0usize;
    while let Ok(completion) = result_rx.try_recv() {
        processed_results = processed_results.saturating_add(1);
        let pid = completion.pid;
        let Some(runtime_id) = runtime_registry
            .runtime_id_for_pid(pid)
            .map(ToString::to_string)
        else {
            tracing::warn!(
                pid,
                "OS: dropping syscall completion for unknown runtime pid"
            );
            continue;
        };
        let should_release_runtime = {
            let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
                tracing::warn!(
                    pid,
                    runtime_id,
                    "OS: dropping syscall completion for unloaded runtime"
                );
                continue;
            };
            let audit_context = AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(&runtime_id),
            );

            if completion.outcome.should_kill_process {
                let _ = engine.inject_context(
                    pid,
                    &engine
                        .format_system_message(&format!("Output:\n{}", completion.outcome.output)),
                );
                kill_managed_process_with_session(
                    engine,
                    memory,
                    scheduler,
                    session_registry,
                    storage,
                    pid,
                    "syscall_killed",
                );
                pending_events.push(KernelEvent::SessionFinished {
                    pid,
                    tokens_generated: None,
                    elapsed_secs: None,
                    reason: "syscall_killed".to_string(),
                });
                pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "syscall_killed".to_string(),
                });
                pending_events.push(KernelEvent::LobbyChanged {
                    reason: "syscall_killed".to_string(),
                });
                audit::record(
                    storage,
                    audit::TOOL_KILLED,
                    format!(
                        "tool_call_id={} command={} caller={} transport=text duration_ms={} success={} detail={}",
                        completion.tool_call_id,
                        completion.command,
                        completion.caller.as_str(),
                        completion.outcome.duration_ms,
                        completion.outcome.success,
                        completion.outcome.output
                    ),
                    audit_context,
                );
                true
            } else {
                match engine.inject_context(
                    pid,
                    &engine
                        .format_system_message(&format!("Output:\n{}", completion.outcome.output)),
                ) {
                    Ok(()) => {
                        if let Some(process) = engine.processes.get_mut(&pid) {
                            process.state = ProcessState::Ready;
                        }
                        pending_events.push(KernelEvent::WorkspaceChanged {
                            pid,
                            reason: "syscall_completed".to_string(),
                        });
                    }
                    Err(err) => {
                        tracing::warn!(pid, %err, "OS: dropping syscall completion for missing process");
                    }
                }
                let spec = if completion.outcome.success {
                    audit::TOOL_COMPLETED
                } else {
                    audit::TOOL_FAILED
                };
                audit::record(
                    storage,
                    spec,
                    format!(
                        "tool_call_id={} command={} caller={} transport=text duration_ms={} detail={}",
                        completion.tool_call_id,
                        completion.command,
                        completion.caller.as_str(),
                        completion.outcome.duration_ms,
                        completion.outcome.output
                    ),
                    audit_context,
                );
                false
            }
        };

        if should_release_runtime {
            if let Err(err) = runtime_registry.release_pid(storage, pid) {
                tracing::warn!(pid, %err, "RUNTIME: failed to release pid after syscall kill");
            }
        }
    }

    processed_results
}

fn dispatch_send_action(
    runtime_id: &str,
    engine: &mut LLMEngine,
    orchestrator: &Orchestrator,
    pid: u64,
    target_pid: Option<u64>,
    target_task: Option<&str>,
    target_role: Option<&str>,
    explicit_orchestration_id: Option<u64>,
    message: Option<&str>,
    payload: Option<&serde_json::Value>,
    message_type: &str,
    channel: Option<&str>,
    session_registry: &SessionRegistry,
    storage: &mut StorageService,
) {
    let normalized_message = message.map(str::trim).filter(|value| !value.is_empty());
    let payload_text = normalized_message
        .map(ToString::to_string)
        .or_else(|| payload.map(|value| value.to_string()))
        .unwrap_or_default();
    if payload_text.is_empty() {
        audit::record(
            storage,
            audit::ACTION_DENIED,
            "action=send reason=empty_payload",
            AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(runtime_id),
            ),
        );
        let _ = engine.inject_context(
            pid,
            &engine.format_system_message(
                "ACTION Error: ACTION:send requires a non-empty message or payload.",
            ),
        );
        return;
    }

    let sender_binding = orchestrator.task_binding_for_pid(pid);
    let sender_role = sender_binding
        .as_ref()
        .and_then(|binding| task_role_for_binding(orchestrator, binding));
    let mut orchestration_id = explicit_orchestration_id
        .or_else(|| sender_binding.as_ref().map(|binding| binding.orch_id));
    let mut receiver_pid = target_pid;
    let mut receiver_task_id = target_task.map(ToString::to_string);
    let mut receiver_attempt = None;
    let mut receiver_role = target_role.map(ToString::to_string);

    if orchestration_id.is_none() {
        orchestration_id = receiver_pid
            .and_then(|candidate_pid| orchestrator.task_binding_for_pid(candidate_pid))
            .map(|binding| binding.orch_id);
    }

    if let Some(task_id) = target_task {
        let Some(orch_id) = orchestration_id else {
            audit::record(
                storage,
                audit::ACTION_DENIED,
                "action=send reason=missing_orchestration_for_task_target",
                AuditContext::for_process(
                    session_registry.session_id_for_pid(pid),
                    pid,
                    Some(runtime_id),
                ),
            );
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(
                    "ACTION Error: task-targeted IPC requires an orchestration context.",
                ),
            );
            return;
        };
        let Some((resolved_pid, resolved_attempt, resolved_role)) =
            resolve_named_task_target(orchestrator, orch_id, task_id)
        else {
            audit::record(
                storage,
                audit::ACTION_DENIED,
                format!("action=send reason=unknown_task_target task={task_id} orch_id={orch_id}"),
                AuditContext::for_process(
                    session_registry.session_id_for_pid(pid),
                    pid,
                    Some(runtime_id),
                ),
            );
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(&format!(
                    "ACTION Error: workflow task '{task_id}' not found in orchestration {orch_id}.",
                )),
            );
            return;
        };
        receiver_pid = receiver_pid.or(resolved_pid);
        receiver_attempt = resolved_attempt;
        receiver_role = receiver_role.or(resolved_role);
    }

    if let Some(role) = target_role {
        let Some(orch_id) = orchestration_id else {
            audit::record(
                storage,
                audit::ACTION_DENIED,
                "action=send reason=missing_orchestration_for_role_target",
                AuditContext::for_process(
                    session_registry.session_id_for_pid(pid),
                    pid,
                    Some(runtime_id),
                ),
            );
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(
                    "ACTION Error: role-targeted IPC requires an orchestration context.",
                ),
            );
            return;
        };
        let Some((resolved_task_id, resolved_pid, resolved_attempt)) =
            resolve_role_target(orchestrator, orch_id, role)
        else {
            audit::record(
                storage,
                audit::ACTION_DENIED,
                format!("action=send reason=unknown_role_target role={role} orch_id={orch_id}"),
                AuditContext::for_process(
                    session_registry.session_id_for_pid(pid),
                    pid,
                    Some(runtime_id),
                ),
            );
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(&format!(
                    "ACTION Error: workflow role '{role}' not found in orchestration {orch_id}.",
                )),
            );
            return;
        };
        receiver_task_id = receiver_task_id.or(Some(resolved_task_id));
        receiver_pid = receiver_pid.or(resolved_pid);
        receiver_attempt = receiver_attempt.or(resolved_attempt);
    }

    let receiver_binding = receiver_pid.and_then(|candidate_pid| {
        let binding = orchestrator.task_binding_for_pid(candidate_pid)?;
        Some(binding)
    });
    if receiver_task_id.is_none() {
        receiver_task_id = receiver_binding
            .as_ref()
            .map(|binding| binding.task_id.clone());
    }
    if receiver_attempt.is_none() {
        receiver_attempt = receiver_binding.as_ref().map(|binding| binding.attempt);
    }
    if receiver_role.is_none() {
        receiver_role = receiver_binding
            .as_ref()
            .and_then(|binding| task_role_for_binding(orchestrator, binding));
    }

    let direct_pid_delivery =
        target_pid.is_some() && target_task.is_none() && target_role.is_none() && channel.is_none();
    let message_id = next_ipc_message_id();
    let preview = build_ipc_payload_preview(&payload_text);
    let _ = storage.record_ipc_message(&crate::storage::NewIpcMessage {
        message_id: message_id.clone(),
        orchestration_id,
        sender_pid: Some(pid),
        sender_task_id: sender_binding
            .as_ref()
            .map(|binding| binding.task_id.clone()),
        sender_attempt: sender_binding.as_ref().map(|binding| binding.attempt),
        receiver_pid,
        receiver_task_id: receiver_task_id.clone(),
        receiver_attempt,
        receiver_role: receiver_role.clone(),
        message_type: message_type.to_string(),
        channel: channel.map(ToString::to_string),
        payload_preview: preview.clone(),
        payload_text: payload_text.clone(),
        status: "queued".to_string(),
    });

    if direct_pid_delivery {
        let Some(target_pid) = target_pid else {
            return;
        };
        let msg_target = engine.format_interprocess_message(pid, &payload_text);
        match engine.inject_context(target_pid, &msg_target) {
            Ok(_) => {
                let _ = storage.update_ipc_message_status(
                    &message_id,
                    "delivered",
                    Some(crate::storage::current_timestamp_ms()),
                    None,
                    None,
                );
                audit::record(
                    storage,
                    audit::ACTION_COMPLETED,
                    format!(
                        "action=send message_id={} type={} target=pid:{} chars={} delivery=direct",
                        message_id,
                        message_type,
                        target_pid,
                        payload_text.chars().count()
                    ),
                    AuditContext::for_process(
                        session_registry.session_id_for_pid(pid),
                        pid,
                        Some(runtime_id),
                    ),
                );
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(
                        "MESSAGE DELIVERED. The target PID received it immediately.",
                    ),
                );
            }
            Err(_) => {
                let _ = storage.update_ipc_message_status(
                    &message_id,
                    "failed",
                    None,
                    None,
                    Some(crate::storage::current_timestamp_ms()),
                );
                audit::record(
                    storage,
                    audit::ACTION_DENIED,
                    format!(
                        "action=send message_id={} type={} reason=target_not_found target_pid={}",
                        message_id, message_type, target_pid
                    ),
                    AuditContext::for_process(
                        session_registry.session_id_for_pid(pid),
                        pid,
                        Some(runtime_id),
                    ),
                );
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(
                        "ACTION Error: target PID not found for direct IPC delivery.",
                    ),
                );
            }
        }
        return;
    }

    let target_summary = receiver_task_id
        .clone()
        .or_else(|| receiver_role.clone().map(|role| format!("role:{role}")))
        .or_else(|| channel.map(|channel| format!("channel:{channel}")))
        .or_else(|| receiver_pid.map(|candidate_pid| format!("pid:{candidate_pid}")))
        .unwrap_or_else(|| "mailbox".to_string());
    audit::record(
        storage,
        audit::ACTION_COMPLETED,
        format!(
            "action=send message_id={} type={} target={} chars={} delivery=queued sender_role={}",
            message_id,
            message_type,
            target_summary,
            payload_text.chars().count(),
            sender_role.unwrap_or_else(|| "-".to_string())
        ),
        AuditContext::for_process(
            session_registry.session_id_for_pid(pid),
            pid,
            Some(runtime_id),
        ),
    );
    let _ = engine.inject_context(
        pid,
        &engine.format_system_message(&format!(
            "MESSAGE QUEUED. message_id={} target={} status=queued. The receiver should use ACTION:receive and ACTION:ack.",
            message_id, target_summary
        )),
    );
}

fn dispatch_receive_action(
    runtime_id: &str,
    engine: &mut LLMEngine,
    orchestrator: &Orchestrator,
    pid: u64,
    limit: usize,
    channel: Option<&str>,
    include_delivered: bool,
    session_registry: &SessionRegistry,
    storage: &mut StorageService,
) {
    let binding = orchestrator.task_binding_for_pid(pid);
    if channel.is_some() && binding.is_none() {
        let _ = engine.inject_context(
            pid,
            &engine.format_system_message(
                "ACTION Error: channel mailbox reads require a workflow task context.",
            ),
        );
        return;
    }

    let selector = crate::storage::IpcMailboxSelector {
        orchestration_id: binding.as_ref().map(|entry| entry.orch_id),
        receiver_pid: Some(pid),
        receiver_task_id: binding.as_ref().map(|entry| entry.task_id.clone()),
        receiver_role: binding
            .as_ref()
            .and_then(|entry| task_role_for_binding(orchestrator, entry)),
        channel: channel.map(ToString::to_string),
    };
    let messages = match storage.load_ipc_mailbox_messages(&selector, include_delivered, limit) {
        Ok(messages) => messages,
        Err(err) => {
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(&format!(
                    "ACTION Error: failed to read mailbox messages: {err}",
                )),
            );
            return;
        }
    };

    if messages.is_empty() {
        audit::record(
            storage,
            audit::ACTION_COMPLETED,
            format!("action=receive count=0 channel={}", channel.unwrap_or("-")),
            AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(runtime_id),
            ),
        );
        let _ = engine.inject_context(
            pid,
            &engine.format_system_message("MAILBOX EMPTY. No pending IPC messages matched."),
        );
        return;
    }

    let delivered_at_ms = crate::storage::current_timestamp_ms();
    for message in &messages {
        if matches!(message.status.as_str(), "queued" | "pending") {
            let _ = storage.update_ipc_message_status(
                &message.message_id,
                "delivered",
                Some(delivered_at_ms),
                None,
                None,
            );
        }
    }

    let mut rendered = format!(
        "MAILBOX RECEIVED {} message(s).\nUse ACTION:ack once you have processed them.\n",
        messages.len()
    );
    for message in &messages {
        rendered.push_str(&format!(
            "\n[message_id={} type={} status={} from={} to={}{}]\n{}\n",
            message.message_id,
            message.message_type,
            if matches!(message.status.as_str(), "queued" | "pending") {
                "delivered"
            } else {
                message.status.as_str()
            },
            format_ipc_sender_label(message),
            format_ipc_receiver_label(message),
            message
                .channel
                .as_ref()
                .map(|value| format!(" channel={value}"))
                .unwrap_or_default(),
            message.payload_text
        ));
    }

    audit::record(
        storage,
        audit::ACTION_COMPLETED,
        format!(
            "action=receive count={} channel={} include_delivered={}",
            messages.len(),
            channel.unwrap_or("-"),
            include_delivered
        ),
        AuditContext::for_process(
            session_registry.session_id_for_pid(pid),
            pid,
            Some(runtime_id),
        ),
    );
    let _ = engine.inject_context(pid, &engine.format_system_message(&rendered));
}

fn dispatch_ack_action(
    runtime_id: &str,
    engine: &mut LLMEngine,
    orchestrator: &Orchestrator,
    pid: u64,
    message_ids: &[String],
    session_registry: &SessionRegistry,
    storage: &mut StorageService,
) {
    let binding = orchestrator.task_binding_for_pid(pid);
    let selector = crate::storage::IpcMailboxSelector {
        orchestration_id: binding.as_ref().map(|entry| entry.orch_id),
        receiver_pid: Some(pid),
        receiver_task_id: binding.as_ref().map(|entry| entry.task_id.clone()),
        receiver_role: binding
            .as_ref()
            .and_then(|entry| task_role_for_binding(orchestrator, entry)),
        channel: None,
    };
    let loaded = match storage.load_ipc_messages_by_ids(message_ids) {
        Ok(messages) => messages,
        Err(err) => {
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(&format!(
                    "ACTION Error: failed to load IPC messages for ack: {err}",
                )),
            );
            return;
        }
    };
    let eligible = loaded
        .into_iter()
        .filter(|message| {
            matches!(message.status.as_str(), "queued" | "pending" | "delivered")
                && (selector.matches(message) || channel_mailbox_match(&selector, message))
        })
        .collect::<Vec<_>>();

    if eligible.is_empty() {
        audit::record(
            storage,
            audit::ACTION_COMPLETED,
            "action=ack count=0",
            AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(runtime_id),
            ),
        );
        let _ = engine.inject_context(
            pid,
            &engine.format_system_message(
                "MAILBOX ACK skipped: no matching messages were eligible for this process mailbox.",
            ),
        );
        return;
    }

    let consumed_at_ms = crate::storage::current_timestamp_ms();
    let acked_ids = eligible
        .iter()
        .map(|message| message.message_id.clone())
        .collect::<Vec<_>>();
    for message_id in &acked_ids {
        let _ = storage.update_ipc_message_status(
            message_id,
            "consumed",
            None,
            Some(consumed_at_ms),
            None,
        );
    }

    audit::record(
        storage,
        audit::ACTION_COMPLETED,
        format!("action=ack count={}", acked_ids.len()),
        AuditContext::for_process(
            session_registry.session_id_for_pid(pid),
            pid,
            Some(runtime_id),
        ),
    );
    let _ = engine.inject_context(
        pid,
        &engine.format_system_message(&format!(
            "MAILBOX ACK completed for {} message(s): {}",
            acked_ids.len(),
            acked_ids.join(", ")
        )),
    );
}
