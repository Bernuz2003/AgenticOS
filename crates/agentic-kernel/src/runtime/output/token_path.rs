use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use agentic_control_models::KernelEvent;
use mio::{Poll, Token};

use crate::backend::InferenceFinishReason;
use crate::diagnostics::audit::{self, AuditContext};
use crate::memory::NeuralMemory;
use crate::orchestrator::Orchestrator;
use crate::process::{AgentProcess, ProcessState};
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::{CheckedOutProcessMetadata, ProcessScheduler};
use crate::services::accounting::{AccountingEventStatus, BackendAccountingEvent};
use crate::services::process_runtime::kill_managed_process_with_session;
use crate::session::SessionRegistry;
use crate::storage::{StorageService, StoredAccountingEvent};
use crate::tool_registry::ToolRegistry;
use crate::transport::Client;

use super::assistant_output::emit_visible_assistant_output;
use super::turn_assembly::TurnAssemblyStore;
use super::turn_completion::emit_turn_completion_events;

pub(super) fn persist_accounting_event(
    storage: &mut StorageService,
    session_registry: &SessionRegistry,
    runtime_registry: &RuntimeRegistry,
    pid: u64,
    runtime_id: &str,
    accounting_event: Option<BackendAccountingEvent>,
) {
    let Some(event) = accounting_event else {
        return;
    };

    let descriptor = runtime_registry.descriptor(runtime_id);
    let session_id = session_registry
        .session_id_for_pid(pid)
        .map(ToString::to_string);
    let record = StoredAccountingEvent {
        session_id,
        pid: Some(pid),
        runtime_id: Some(runtime_id.to_string()),
        backend_id: descriptor
            .map(|runtime| runtime.backend_id.clone())
            .unwrap_or_else(|| event.backend_id.clone()),
        backend_class: descriptor
            .map(|runtime| runtime.backend_class.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        provider_id: descriptor.and_then(|runtime| runtime.provider_id.clone()),
        model_id: descriptor
            .and_then(|runtime| runtime.remote_model_id.clone())
            .or(event.model_id.clone())
            .or_else(|| descriptor.map(|runtime| runtime.logical_model_id.clone())),
        request_kind: "inference_step".to_string(),
        status: event.status,
        request_count: event.request_count,
        stream: event.stream,
        input_tokens: event.input_tokens,
        output_tokens: event.output_tokens,
        estimated_cost_usd: event.estimated_cost_usd,
        error_code: event.error_code,
        error_message: event.error_message,
    };

    if let Err(err) = storage.record_accounting_event(&record) {
        tracing::warn!(
            pid,
            runtime_id,
            %err,
            "ACCOUNTING: failed to persist request accounting event"
        );
        return;
    }

    let audit_context =
        AuditContext::for_process(record.session_id.as_deref(), pid, Some(runtime_id));
    audit::record(
        storage,
        audit::ACCOUNTING_USAGE_RECORDED,
        format!(
            "request_kind={} status={} model={} tokens={}/{} cost=${:.6} duration_ms={}",
            record.request_kind,
            record.status.as_str(),
            record.model_id.as_deref().unwrap_or("unknown"),
            record.input_tokens,
            record.output_tokens,
            record.estimated_cost_usd,
            event.duration_ms
        ),
        audit_context.clone(),
    );
    if record.estimated_cost_usd > 0.0 {
        audit::record(
            storage,
            audit::ACCOUNTING_COST_RECORDED,
            format!(
                "model={} cost=${:.6} backend={}",
                record.model_id.as_deref().unwrap_or("unknown"),
                record.estimated_cost_usd,
                record.backend_id
            ),
            audit_context.clone(),
        );
    }

    if record.backend_class == "remote_stateless" {
        let remote_spec = if matches!(record.status, AccountingEventStatus::Success) {
            audit::REMOTE_REQUEST_COMPLETED
        } else {
            audit::REMOTE_REQUEST_FAILED
        };
        audit::record(
            storage,
            remote_spec,
            format!(
                "backend={} model={} status={} duration_ms={} tokens={}/{} error={}",
                record.backend_id,
                record.model_id.as_deref().unwrap_or("unknown"),
                record.status.as_str(),
                event.duration_ms,
                record.input_tokens,
                record.output_tokens,
                record.error_message.as_deref().unwrap_or("none")
            ),
            audit_context,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_token_result(
    runtime_registry: &mut RuntimeRegistry,
    memory: &mut NeuralMemory,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    pid: u64,
    mut process: AgentProcess,
    text_output: String,
    generated_tokens: usize,
    finished: bool,
    finish_reason: Option<InferenceFinishReason>,
    accounting_event: Option<BackendAccountingEvent>,
    syscall_cmd_tx: &mpsc::Sender<crate::runtime::syscalls::SyscallCmd>,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    turn_assembly: &mut TurnAssemblyStore,
    runtime_checked_out: Option<CheckedOutProcessMetadata>,
    in_flight: &mut HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pending_events: &mut Vec<KernelEvent>,
    tool_registry: &ToolRegistry,
) {
    in_flight.remove(&pid);
    let Some(runtime_id) = runtime_registry
        .runtime_id_for_pid(pid)
        .map(ToString::to_string)
    else {
        tracing::warn!(
            pid,
            "RUNTIME: dropping worker token for unknown runtime pid"
        );
        return;
    };
    persist_accounting_event(
        storage,
        session_registry,
        runtime_registry,
        pid,
        &runtime_id,
        accounting_event,
    );

    let owner_id_from_checkout = runtime_checked_out
        .as_ref()
        .map(|metadata| metadata.owner_id)
        .unwrap_or(0);
    let finalized_step = turn_assembly.consume_final_fragment(pid, &text_output);
    let pid_floor = runtime_registry.next_pid_floor();
    let audit_context = AuditContext::for_process(
        session_registry.session_id_for_pid(pid),
        pid,
        Some(&runtime_id),
    );

    let syscall_dispatch = {
        let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
            tracing::warn!(
                pid,
                runtime_id,
                "RUNTIME: runtime missing engine for worker token"
            );
            return;
        };

        if generated_tokens > 0 || !finalized_step.complete_assistant_text.is_empty() {
            process.record_model_output(&finalized_step.complete_assistant_text, generated_tokens);
        }

        if !finished
            && !text_output.is_empty()
            && crate::prompting::should_stop_on_text_with_metadata(
                engine.family,
                &text_output,
                engine.model_metadata(),
            )
        {
            process.state = if process.lifecycle_policy.is_interactive() {
                ProcessState::WaitingForInput
            } else {
                ProcessState::Finished
            };
            process.termination_reason = Some("stop_marker_detected".to_string());
        } else if finished {
            process.termination_reason = Some(
                finish_reason
                    .unwrap_or(InferenceFinishReason::ModelStop)
                    .as_str()
                    .to_string(),
            );
        }

        process.mark_resident_prompt_checkpoint();
        engine.processes.insert(pid, process);

        if pending_kills.contains(&pid) {
            pending_kills.retain(|&queued_pid| queued_pid != pid);
            kill_managed_process_with_session(
                engine,
                memory,
                scheduler,
                session_registry,
                storage,
                pid,
                "killed",
            );
            audit::record(
                storage,
                audit::PROCESS_KILLED,
                "reason=queued_kill_in_flight",
                audit_context.clone(),
            );
            crate::runtime::syscalls::SyscallDispatchOutcome::Killed
        } else {
            let owner_id = engine
                .process_owner_id(pid)
                .unwrap_or(owner_id_from_checkout);
            emit_visible_assistant_output(
                pid,
                owner_id,
                &finalized_step.visible_text,
                clients,
                poll,
                orchestrator,
                pending_events,
                "model_output",
            );
            let pending_syscall = finalized_step.syscall_command.clone();

            if let Some(full_command) = pending_syscall {
                let content = full_command.trim().to_string();
                tracing::info!(pid, owner_id, command = %full_command, "OS: SysCall intercepted");
                crate::runtime::syscalls::dispatch_process_syscall(
                    &runtime_id,
                    pid_floor,
                    engine,
                    memory,
                    scheduler,
                    orchestrator,
                    pid,
                    &content,
                    syscall_cmd_tx,
                    session_registry,
                    storage,
                    pending_events,
                    tool_registry,
                )
            } else {
                crate::runtime::syscalls::SyscallDispatchOutcome::None
            }
        }
    };

    if matches!(
        syscall_dispatch,
        crate::runtime::syscalls::SyscallDispatchOutcome::Killed
    ) {
        if let Err(err) = runtime_registry.release_pid(storage, pid) {
            tracing::warn!(pid, %err, "RUNTIME: failed to release pid after queued kill");
        }
        return;
    }

    let token_quota_exceeded = (0..generated_tokens).any(|_| scheduler.record_token(pid));

    if let crate::runtime::syscalls::SyscallDispatchOutcome::Spawned(spawned_pid) = syscall_dispatch
    {
        if let Err(err) = runtime_registry.register_pid(storage, &runtime_id, spawned_pid) {
            tracing::warn!(
                pid = spawned_pid,
                runtime_id,
                %err,
                "RUNTIME: failed to register syscall-spawned pid"
            );
        }
    }

    if token_quota_exceeded {
        tracing::warn!(pid, "SCHEDULER: token quota exceeded — terminating process");
        if let Some(engine) = runtime_registry.engine_mut(&runtime_id) {
            if let Some(process) = engine.processes.get_mut(&pid) {
                process.state = ProcessState::Finished;
                process.termination_reason = Some("token_quota_reached".to_string());
            }
        }
    }

    emit_turn_completion_events(
        runtime_registry,
        scheduler,
        pid,
        &runtime_id,
        syscall_dispatch,
        pending_events,
        storage,
        audit_context,
    );
}
