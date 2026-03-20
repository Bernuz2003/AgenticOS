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
            }
        }
        SyscallDispatchOutcome::None
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
        process.state = ProcessState::WaitingForInput;
    }
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
            let Some(target_pid) = invocation.input.get("pid").and_then(|value| value.as_u64())
            else {
                audit::record(
                    storage,
                    audit::ACTION_DENIED,
                    "action=send reason=missing_pid",
                    AuditContext::for_process(
                        session_registry.session_id_for_pid(pid),
                        pid,
                        Some(runtime_id),
                    ),
                );
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(
                        "ACTION Error: ACTION:send requires numeric field 'pid'.",
                    ),
                );
                return SyscallDispatchOutcome::None;
            };
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
            let channel = invocation
                .input
                .get("channel")
                .and_then(|value| value.as_str());
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
                message.as_deref(),
                payload.as_ref(),
                message_type,
                channel,
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
    target_pid: u64,
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
            &engine
                .format_system_message(
                    "ACTION Error: ACTION:send requires a non-empty message or payload.",
                ),
        );
        return;
    }

    let sender_binding = orchestrator.task_binding_for_pid(pid);
    let receiver_binding = orchestrator.task_binding_for_pid(target_pid);
    let orchestration_id = match (sender_binding.as_ref(), receiver_binding.as_ref()) {
        (Some(sender), Some(receiver)) if sender.orch_id == receiver.orch_id => Some(sender.orch_id),
        (Some(sender), _) => Some(sender.orch_id),
        (_, Some(receiver)) => Some(receiver.orch_id),
        _ => None,
    };
    let message_id = next_ipc_message_id();
    let preview = if payload_text.chars().count() > 240 {
        let mut preview = payload_text.chars().take(240).collect::<String>();
        preview.push_str("...");
        preview
    } else {
        payload_text.clone()
    };
    let _ = storage.record_ipc_message(&crate::storage::NewIpcMessage {
        message_id: message_id.clone(),
        orchestration_id,
        sender_pid: Some(pid),
        sender_task_id: sender_binding.as_ref().map(|binding| binding.task_id.clone()),
        sender_attempt: sender_binding.as_ref().map(|binding| binding.attempt),
        receiver_pid: Some(target_pid),
        receiver_task_id: receiver_binding.as_ref().map(|binding| binding.task_id.clone()),
        receiver_attempt: receiver_binding.as_ref().map(|binding| binding.attempt),
        message_type: message_type.to_string(),
        channel: channel.map(ToString::to_string),
        payload_preview: preview.clone(),
        payload_text: payload_text.clone(),
        status: "pending".to_string(),
    });

    let msg_target = engine.format_interprocess_message(pid, &payload_text);
    match engine.inject_context(target_pid, &msg_target) {
        Ok(_) => {
            let _ = storage.update_ipc_message_delivery(
                &message_id,
                "delivered",
                Some(crate::storage::current_timestamp_ms()),
                None,
            );
            audit::record(
                storage,
                audit::ACTION_COMPLETED,
                format!(
                    "action=send message_id={} type={} channel={} target_pid={} chars={}",
                    message_id,
                    message_type,
                    channel.unwrap_or("-"),
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
                    "MESSAGE SENT. Waiting for reply... (Do not send again).",
                ),
            );
        }
        Err(_) => {
            let _ = storage.update_ipc_message_delivery(&message_id, "failed", None, None);
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
                &engine
                    .format_system_message("ERROR: Target PID not found (Process does not exist)."),
            );
        }
    }
}
