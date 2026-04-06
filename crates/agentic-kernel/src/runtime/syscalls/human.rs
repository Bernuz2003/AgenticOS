use serde_json::json;

use crate::core_dump::{invocation_marker, record_live_debug_checkpoint};
use crate::diagnostics::audit::{self, AuditContext};
use crate::engine::LLMEngine;
use crate::process::HumanInputRequest;
use crate::process::ProcessState;
use crate::runtime::TurnAssemblyStore;
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tools::human_tools::{build_approval_request, normalize_ask_human_request, AskHumanInput};
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};
use agentic_control_models::{InvocationKind, InvocationStatus, KernelEvent};

use super::dispatch::SyscallDispatchOutcome;
use super::invocation_events::emit_invocation_updated;
use super::tool_history::{
    complete_tool_invocation, record_tool_invocation_dispatched, ToolInvocationCompletionData,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_native_human_input_request(
    runtime_id: &str,
    engine: &mut LLMEngine,
    pid: u64,
    content: &str,
    tool_call_id: &str,
    caller: &ToolCaller,
    permissions: &ProcessPermissionPolicy,
    turn_assembly: &TurnAssemblyStore,
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
    emit_invocation_updated(
        pending_events,
        pid,
        tool_call_id,
        InvocationKind::Tool,
        content,
        InvocationStatus::Dispatched,
    );
    if let Err(err) = record_tool_invocation_dispatched(
        storage,
        session_registry,
        runtime_id,
        pid,
        tool_call_id,
        content,
        caller,
    ) {
        tracing::warn!(
            pid,
            tool_call_id,
            %err,
            "FORENSICS: failed to persist ask_human dispatch"
        );
    }

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
        emit_invocation_updated(
            pending_events,
            pid,
            tool_call_id,
            InvocationKind::Tool,
            content,
            InvocationStatus::Failed,
        );
        if let Err(err) = complete_tool_invocation(
            storage,
            tool_call_id,
            "failed",
            ToolInvocationCompletionData {
                output_json: Some(json!({
                    "output": "TOOL Error: ask_human is not allowed for this process policy.",
                })),
                output_text: Some(
                    "TOOL Error: ask_human is not allowed for this process policy.".to_string(),
                ),
                error_kind: Some("policy_denied".to_string()),
                error_text: Some("ask_human is not allowed for this process policy.".to_string()),
                ..ToolInvocationCompletionData::default()
            },
        ) {
            tracing::warn!(
                pid,
                tool_call_id,
                %err,
                "FORENSICS: failed to persist ask_human policy failure"
            );
        }
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
            emit_invocation_updated(
                pending_events,
                pid,
                tool_call_id,
                InvocationKind::Tool,
                content,
                InvocationStatus::Failed,
            );
            if let Err(history_err) = complete_tool_invocation(
                storage,
                tool_call_id,
                "failed",
                ToolInvocationCompletionData {
                    output_json: Some(json!({
                        "output": format!("TOOL Error: invalid ask_human payload: {err}"),
                    })),
                    output_text: Some(format!("TOOL Error: invalid ask_human payload: {err}")),
                    error_kind: Some("invalid_input".to_string()),
                    error_text: Some(err.to_string()),
                    ..ToolInvocationCompletionData::default()
                },
            ) {
                tracing::warn!(
                    pid,
                    tool_call_id,
                    %history_err,
                    "FORENSICS: failed to persist ask_human payload failure"
                );
            }
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
            emit_invocation_updated(
                pending_events,
                pid,
                tool_call_id,
                InvocationKind::Tool,
                content,
                InvocationStatus::Failed,
            );
            if let Err(history_err) = complete_tool_invocation(
                storage,
                tool_call_id,
                "failed",
                ToolInvocationCompletionData {
                    output_json: Some(json!({
                        "output": format!("TOOL Error: {}", err),
                    })),
                    output_text: Some(format!("TOOL Error: {}", err)),
                    error_kind: Some("invalid_input".to_string()),
                    error_text: Some(err.to_string()),
                    ..ToolInvocationCompletionData::default()
                },
            ) {
                tracing::warn!(
                    pid,
                    tool_call_id,
                    %history_err,
                    "FORENSICS: failed to persist ask_human normalization failure"
                );
            }
            return Some(SyscallDispatchOutcome::None);
        }
    };

    let question = request.question.clone();
    let kind = request.kind.as_str().to_string();
    let choices = request.choices.join("|");
    park_process_for_human_request(
        runtime_id,
        engine,
        pid,
        content,
        tool_call_id,
        caller,
        turn_assembly,
        session_registry,
        storage,
        pending_events,
        request,
        json!({
            "output": "pending_human_input",
            "request": {
                "question": question,
                "kind": kind,
                "choices": choices,
            }
        }),
        "human_input_requested",
    );
    Some(SyscallDispatchOutcome::None)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_mcp_tool_approval_request(
    runtime_id: &str,
    engine: &mut LLMEngine,
    pid: u64,
    content: &str,
    tool_call_id: &str,
    caller: &ToolCaller,
    turn_assembly: &TurnAssemblyStore,
    session_registry: &SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
    tool_name: &str,
    server_id: &str,
    target_name: &str,
    trust_level: &str,
) -> SyscallDispatchOutcome {
    let request = build_approval_request(
        format!("Approve MCP tool '{}'?", tool_name),
        Some(format!(
            "provider=mcp server_id={} target_name={} trust_level={} command={}",
            server_id, target_name, trust_level, content
        )),
    );
    park_process_for_human_request(
        runtime_id,
        engine,
        pid,
        content,
        tool_call_id,
        caller,
        turn_assembly,
        session_registry,
        storage,
        pending_events,
        request,
        json!({
            "output": "pending_human_input",
            "approval_required": true,
            "provider": "mcp",
            "tool_name": tool_name,
            "server_id": server_id,
            "target_name": target_name,
            "trust_level": trust_level,
        }),
        "mcp_tool_approval_requested",
    )
}

#[allow(clippy::too_many_arguments)]
fn park_process_for_human_request(
    runtime_id: &str,
    engine: &mut LLMEngine,
    pid: u64,
    content: &str,
    tool_call_id: &str,
    caller: &ToolCaller,
    turn_assembly: &TurnAssemblyStore,
    session_registry: &SessionRegistry,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
    request: HumanInputRequest,
    completion_payload: serde_json::Value,
    effect_kind: &str,
) -> SyscallDispatchOutcome {
    let audit_context = AuditContext::for_process(
        session_registry.session_id_for_pid(pid),
        pid,
        Some(runtime_id),
    );
    let question = request.question.clone();
    let kind = request.kind.as_str().to_string();
    let choices = request.choices.join("|");
    let request_snapshot = request.clone();
    if let Some(process) = engine.processes.get_mut(&pid) {
        process.set_pending_human_request(request);
        process.state = ProcessState::WaitingForHumanInput;
        if let Err(err) = record_live_debug_checkpoint(
            storage,
            session_registry,
            turn_assembly,
            runtime_id,
            pid,
            process,
            "human_input_requested",
            invocation_marker(Some(tool_call_id), Some(content), Some("completed")),
        ) {
            tracing::debug!(
                pid,
                tool_call_id,
                %err,
                "FORENSICS: skipped pending human request checkpoint without turn assembly context"
            );
        }
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
            "tool_call_id={} command={} caller={} transport=text detail=pending_human_input effect_kind={}",
            tool_call_id,
            content,
            caller.as_str(),
            effect_kind
        ),
        audit_context,
    );
    emit_invocation_updated(
        pending_events,
        pid,
        tool_call_id,
        InvocationKind::Tool,
        content,
        InvocationStatus::Completed,
    );
    if let Err(err) = complete_tool_invocation(
        storage,
        tool_call_id,
        "completed",
        ToolInvocationCompletionData {
            output_json: Some(json!({
                "request": request_snapshot,
                "output": "pending_human_input",
                "effect_kind": effect_kind,
                "payload": completion_payload,
            })),
            output_text: Some("pending_human_input".to_string()),
            effects: vec![json!({
                "kind": effect_kind,
                "request_id": request_snapshot.request_id,
            })],
            ..ToolInvocationCompletionData::default()
        },
    ) {
        tracing::warn!(
            pid,
            tool_call_id,
            %err,
            "FORENSICS: failed to persist pending human request completion"
        );
    }
    SyscallDispatchOutcome::None
}
