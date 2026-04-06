use std::sync::mpsc;

use serde_json::json;

use agentic_control_models::{InvocationKind, InvocationStatus, KernelEvent};

use crate::core_dump::{invocation_marker, record_live_debug_checkpoint};
use crate::diagnostics::audit::{self, AuditContext};
use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::WorkloadClass;
use crate::orchestrator::Orchestrator;
use crate::process::{ProcessLifecyclePolicy, ProcessState};
use crate::runtime::TurnAssemblyStore;
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::services::process_runtime::{
    kill_managed_process_with_session, spawn_managed_process_with_session, ManagedProcessRequest,
};
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::tools::invocation::{
    PathGrantAccessMode, ProcessPathGrant, ProcessPermissionPolicy, ProcessTrustScope, ToolCaller,
};

use super::human::{dispatch_mcp_tool_approval_request, dispatch_native_human_input_request};
use super::ids::{next_action_call_id, next_tool_call_id};
use super::invocation_events::emit_invocation_updated;
use super::ipc::{
    dispatch_ack_action, dispatch_receive_action, dispatch_send_action, non_empty_input_str,
};
use super::parser::{self, ActionInvocation, ActionName};
use super::replay::replay_stubbed_completion;
use super::tool_history::{
    complete_tool_invocation, record_tool_invocation_dispatched, ToolInvocationCompletionData,
};
use super::worker::SyscallCmd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyscallDispatchOutcome {
    None,
    Queued,
    Spawned(u64),
    Killed,
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch_process_syscall(
    runtime_id: &str,
    pid_floor: u64,
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    orchestrator: &Orchestrator,
    pid: u64,
    content: &str,
    turn_assembly: &TurnAssemblyStore,
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
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: Vec::new(),
            path_grants: vec![ProcessPathGrant {
                root: ".".to_string(),
                access_mode: PathGrantAccessMode::AutonomousWrite,
                capsule: Some("workspace".to_string()),
                label: Some("Workspace".to_string()),
            }],
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
        let action_call_id = next_action_call_id(pid);
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
            emit_invocation_updated(
                pending_events,
                pid,
                action_call_id,
                InvocationKind::Action,
                content,
                InvocationStatus::Failed,
            );
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(
                    "ACTION Error: this session is interactive and cannot orchestrate other agents.",
                ),
            );
            return SyscallDispatchOutcome::None;
        }

        match parser::parse_text_invocation(content) {
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
                emit_invocation_updated(
                    pending_events,
                    pid,
                    action_call_id.clone(),
                    InvocationKind::Action,
                    content,
                    InvocationStatus::Dispatched,
                );
                dispatch_action_invocation(
                    runtime_id,
                    pid_floor,
                    engine,
                    memory,
                    scheduler,
                    orchestrator,
                    pid,
                    &action_call_id,
                    content,
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
                emit_invocation_updated(
                    pending_events,
                    pid,
                    action_call_id,
                    InvocationKind::Action,
                    content,
                    InvocationStatus::Failed,
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
            turn_assembly,
            session_registry,
            storage,
            pending_events,
        ) {
            return outcome;
        }
        emit_invocation_updated(
            pending_events,
            pid,
            tool_call_id.clone(),
            InvocationKind::Tool,
            content,
            InvocationStatus::Dispatched,
        );
        if let Err(err) = record_tool_invocation_dispatched(
            storage,
            session_registry,
            runtime_id,
            pid,
            &tool_call_id,
            content,
            &caller,
        ) {
            tracing::warn!(
                pid,
                tool_call_id,
                %err,
                "FORENSICS: failed to persist tool invocation dispatch"
            );
        }
        audit::record(
            storage,
            audit::TOOL_DISPATCHED,
            format!(
                "tool_call_id={} command={} caller={} transport=text{}",
                tool_call_id,
                content,
                caller.as_str(),
                tool_audit_suffix(tool_registry, extract_tool_name(content))
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
        if let Some((tool_name, server_id, target_name, trust_level)) =
            mcp_tool_requiring_approval(tool_registry, extract_tool_name(content))
        {
            return dispatch_mcp_tool_approval_request(
                runtime_id,
                engine,
                pid,
                content,
                &tool_call_id,
                &caller,
                turn_assembly,
                session_registry,
                storage,
                pending_events,
                &tool_name,
                &server_id,
                &target_name,
                &trust_level,
            );
        }
        let queued_command =
            replay_stubbed_completion(scheduler, pid, &tool_call_id, content, &caller)
                .map(|completion| SyscallCmd::ReplayCompletion { completion })
                .unwrap_or_else(|| SyscallCmd::Execute {
                    pid,
                    tool_call_id: tool_call_id.clone(),
                    content: content.to_string(),
                    caller,
                    permissions,
                    registry: tool_registry.clone(),
                });
        let queued = syscall_cmd_tx.send(queued_command);
        match queued {
            Ok(()) => {
                if let Some(process) = engine.processes.get_mut(&pid) {
                    process.state = ProcessState::WaitingForSyscall;
                    if let Err(err) = record_live_debug_checkpoint(
                        storage,
                        session_registry,
                        turn_assembly,
                        runtime_id,
                        pid,
                        process,
                        "syscall_dispatched",
                        invocation_marker(Some(&tool_call_id), Some(content), Some("dispatched")),
                    ) {
                        tracing::debug!(
                            pid,
                            tool_call_id,
                            %err,
                            "FORENSICS: skipped dispatch checkpoint without turn assembly context"
                        );
                    }
                }
                pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "syscall_dispatched".to_string(),
                });
                SyscallDispatchOutcome::Queued
            }
            Err(err) => {
                if let Err(history_err) = complete_tool_invocation(
                    storage,
                    &tool_call_id,
                    "failed",
                    ToolInvocationCompletionData {
                        output_json: Some(json!({
                            "output": format!("SysCall Error: failed to enqueue execution: {}", err),
                        })),
                        output_text: Some(format!(
                            "SysCall Error: failed to enqueue execution: {}",
                            err
                        )),
                        error_kind: Some("queue_failed".to_string()),
                        error_text: Some(err.to_string()),
                        ..ToolInvocationCompletionData::default()
                    },
                ) {
                    tracing::warn!(
                        pid,
                        tool_call_id,
                        %history_err,
                        "FORENSICS: failed to persist tool invocation queue failure"
                    );
                }
                emit_invocation_updated(
                    pending_events,
                    pid,
                    tool_call_id,
                    InvocationKind::Tool,
                    content,
                    InvocationStatus::Failed,
                );
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

fn tool_audit_suffix(tool_registry: &ToolRegistry, tool_name: Option<&str>) -> String {
    let Some(tool_name) = tool_name else {
        return String::new();
    };
    let Some(entry) = tool_registry.get(tool_name) else {
        return String::new();
    };
    let Some(interop) = entry.descriptor.interop.as_ref() else {
        return String::new();
    };
    if interop.provider != "mcp" {
        return String::new();
    }

    format!(
        " provider={} mcp_server={} mcp_tool={} trust_level={} approval_required={}",
        interop.provider,
        interop.server_id,
        interop.target_name,
        interop.trust_level,
        interop.approval_required
    )
}

fn mcp_tool_requiring_approval(
    tool_registry: &ToolRegistry,
    tool_name: Option<&str>,
) -> Option<(String, String, String, String)> {
    let tool_name = tool_name?;
    let entry = tool_registry.get(tool_name)?;
    let interop = entry.descriptor.interop.as_ref()?;
    if interop.provider != "mcp" || !interop.approval_required {
        return None;
    }

    Some((
        entry.descriptor.name.clone(),
        interop.server_id.clone(),
        interop.target_name.clone(),
        interop.trust_level.clone(),
    ))
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
    invocation_id: &str,
    command: &str,
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
                emit_invocation_updated(
                    pending_events,
                    pid,
                    invocation_id,
                    InvocationKind::Action,
                    command,
                    InvocationStatus::Failed,
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
                invocation_id,
                command,
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
                emit_invocation_updated(
                    pending_events,
                    pid,
                    invocation_id,
                    InvocationKind::Action,
                    command,
                    InvocationStatus::Failed,
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
                emit_invocation_updated(
                    pending_events,
                    pid,
                    invocation_id,
                    InvocationKind::Action,
                    command,
                    InvocationStatus::Failed,
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
            emit_invocation_updated(
                pending_events,
                pid,
                invocation_id,
                InvocationKind::Action,
                command,
                InvocationStatus::Completed,
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
            emit_invocation_updated(
                pending_events,
                pid,
                invocation_id,
                InvocationKind::Action,
                command,
                InvocationStatus::Completed,
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
                emit_invocation_updated(
                    pending_events,
                    pid,
                    invocation_id,
                    InvocationKind::Action,
                    command,
                    InvocationStatus::Failed,
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
            emit_invocation_updated(
                pending_events,
                pid,
                invocation_id,
                InvocationKind::Action,
                command,
                InvocationStatus::Completed,
            );
            SyscallDispatchOutcome::None
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{mcp_tool_requiring_approval, tool_audit_suffix};
    use crate::tool_registry::{
        ToolBackendConfig, ToolBackendKind, ToolDescriptor, ToolInteropDescriptor,
        ToolInteropHints, ToolRegistry, ToolRegistryEntry, ToolSource,
    };
    use crate::tools::invocation::ToolCaller;

    #[test]
    fn includes_mcp_metadata_in_tool_dispatch_audit_suffix() {
        let mut registry = ToolRegistry::new();
        registry
            .register(ToolRegistryEntry {
                descriptor: ToolDescriptor {
                    name: "demo.echo".to_string(),
                    aliases: Vec::new(),
                    description: "MCP echo".to_string(),
                    input_schema: json!({"type": "object"}),
                    input_example: None,
                    output_schema: json!({"type": "object"}),
                    allowed_callers: vec![ToolCaller::AgentText],
                    backend_kind: ToolBackendKind::RemoteHttp,
                    capabilities: vec!["mcp".to_string()],
                    dangerous: false,
                    enabled: true,
                    default_allowlisted: false,
                    approval_required: false,
                    interop: Some(ToolInteropDescriptor {
                        provider: "mcp".to_string(),
                        server_id: "demo".to_string(),
                        server_label: Some("Demo".to_string()),
                        transport: "stdio".to_string(),
                        target_name: "echo".to_string(),
                        trust_level: "trusted".to_string(),
                        auth_mode: "environment".to_string(),
                        default_allowlisted: false,
                        approval_required: false,
                        hints: ToolInteropHints::default(),
                    }),
                    source: ToolSource::Runtime,
                },
                backend: ToolBackendConfig::RemoteHttp {
                    url: "http://127.0.0.1:1/demo".to_string(),
                    method: "POST".to_string(),
                    timeout_ms: 100,
                    headers: Default::default(),
                },
            })
            .expect("register tool");

        let suffix = tool_audit_suffix(&registry, Some("demo.echo"));

        assert_eq!(
            suffix,
            " provider=mcp mcp_server=demo mcp_tool=echo trust_level=trusted approval_required=false"
        );
    }

    #[test]
    fn detects_mcp_tools_that_require_approval() {
        let mut registry = ToolRegistry::new();
        registry
            .register(ToolRegistryEntry {
                descriptor: ToolDescriptor {
                    name: "demo.echo".to_string(),
                    aliases: Vec::new(),
                    description: "MCP echo".to_string(),
                    input_schema: json!({"type": "object"}),
                    input_example: None,
                    output_schema: json!({"type": "object"}),
                    allowed_callers: vec![ToolCaller::AgentText],
                    backend_kind: ToolBackendKind::RemoteHttp,
                    capabilities: vec!["mcp".to_string()],
                    dangerous: false,
                    enabled: true,
                    default_allowlisted: false,
                    approval_required: true,
                    interop: Some(ToolInteropDescriptor {
                        provider: "mcp".to_string(),
                        server_id: "demo".to_string(),
                        server_label: Some("Demo".to_string()),
                        transport: "stdio".to_string(),
                        target_name: "echo".to_string(),
                        trust_level: "trusted".to_string(),
                        auth_mode: "environment".to_string(),
                        default_allowlisted: false,
                        approval_required: true,
                        hints: ToolInteropHints::default(),
                    }),
                    source: ToolSource::Runtime,
                },
                backend: ToolBackendConfig::RemoteHttp {
                    url: "http://127.0.0.1:1/demo".to_string(),
                    method: "POST".to_string(),
                    timeout_ms: 100,
                    headers: Default::default(),
                },
            })
            .expect("register tool");

        let approval = mcp_tool_requiring_approval(&registry, Some("demo.echo"))
            .expect("approval-required tool");

        assert_eq!(
            approval,
            (
                "demo.echo".to_string(),
                "demo".to_string(),
                "echo".to_string(),
                "trusted".to_string(),
            )
        );
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
    invocation_id: &str,
    command: &str,
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
        emit_invocation_updated(
            pending_events,
            pid,
            invocation_id,
            InvocationKind::Action,
            command,
            InvocationStatus::Failed,
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
        .or_else(|| ProcessPermissionPolicy::interactive_chat(tool_registry).ok());

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
        emit_invocation_updated(
            pending_events,
            pid,
            invocation_id,
            InvocationKind::Action,
            command,
            InvocationStatus::Failed,
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
            system_prompt: Some(
                crate::agent_prompt::build_agent_system_prompt_with_allowed_tools(
                    tool_registry,
                    ToolCaller::AgentText,
                    child_permission_policy
                        .as_ref()
                        .map(|policy| policy.allowed_tools.as_slice()),
                ),
            ),
            owner_id,
            tool_caller: ToolCaller::AgentText,
            permission_policy: child_permission_policy,
            workload,
            required_backend_class: None,
            priority,
            lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
            context_policy: inherited_context_policy,
            quota_override: None,
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
            emit_invocation_updated(
                pending_events,
                pid,
                invocation_id,
                InvocationKind::Action,
                command,
                InvocationStatus::Completed,
            );
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
            emit_invocation_updated(
                pending_events,
                pid,
                invocation_id,
                InvocationKind::Action,
                command,
                InvocationStatus::Failed,
            );
            let _ =
                engine.inject_context(pid, &engine.format_system_message(&format!("ERROR: {}", e)));
            SyscallDispatchOutcome::None
        }
    }
}
