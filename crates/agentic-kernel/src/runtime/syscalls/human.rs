use crate::diagnostics::audit::{self, AuditContext};
use crate::engine::LLMEngine;
use crate::process::ProcessState;
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tools::human_tools::{normalize_ask_human_request, AskHumanInput};
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};
use agentic_control_models::{InvocationKind, InvocationStatus, KernelEvent};

use super::dispatch::SyscallDispatchOutcome;
use super::invocation_events::emit_invocation_updated;

#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_native_human_input_request(
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
    emit_invocation_updated(
        pending_events,
        pid,
        tool_call_id,
        InvocationKind::Tool,
        content,
        InvocationStatus::Dispatched,
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
        emit_invocation_updated(
            pending_events,
            pid,
            tool_call_id,
            InvocationKind::Tool,
            content,
            InvocationStatus::Failed,
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
            emit_invocation_updated(
                pending_events,
                pid,
                tool_call_id,
                InvocationKind::Tool,
                content,
                InvocationStatus::Failed,
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
            emit_invocation_updated(
                pending_events,
                pid,
                tool_call_id,
                InvocationKind::Tool,
                content,
                InvocationStatus::Failed,
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
    emit_invocation_updated(
        pending_events,
        pid,
        tool_call_id,
        InvocationKind::Tool,
        content,
        InvocationStatus::Completed,
    );
    Some(SyscallDispatchOutcome::None)
}
