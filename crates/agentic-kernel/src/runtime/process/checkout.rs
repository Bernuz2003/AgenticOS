use std::collections::HashSet;
use std::sync::mpsc;

use crate::diagnostics::audit::{self, AuditContext};
use crate::inference_worker::InferenceCmd;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::{CheckedOutProcessMetadata, ProcessScheduler};
use crate::session::SessionRegistry;
use crate::storage::StorageService;

use super::waiting_states::{checked_out_state_label, is_checkout_eligible};

pub(crate) fn checkout_active_processes(
    runtime_registry: &mut RuntimeRegistry,
    scheduler: &mut ProcessScheduler,
    cmd_tx: &mpsc::Sender<InferenceCmd>,
    in_flight: &mut HashSet<u64>,
    session_registry: &SessionRegistry,
    storage: &mut StorageService,
) -> usize {
    let active_pids = runtime_registry.all_active_pids();
    let ordered_pids = scheduler.scheduling_order(&active_pids);
    let mut checked_out_count = 0usize;

    for pid in ordered_pids {
        if in_flight.contains(&pid) {
            continue;
        }
        let Some(runtime_id) = runtime_registry
            .runtime_id_for_pid(pid)
            .map(ToString::to_string)
        else {
            continue;
        };
        let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
            continue;
        };
        let eos = engine.eos_token_id;
        let eot = engine.eot_token_id;
        if let Some(mut process) = engine.processes.remove(&pid) {
            if !is_checkout_eligible(&process.state) {
                engine.processes.insert(pid, process);
                continue;
            }
            if let Some(event) = process.enforce_context_budget() {
                tracing::info!(
                    pid,
                    strategy = event.strategy.label(),
                    dropped_segments = event.dropped_segments,
                    dropped_tokens = event.dropped_tokens,
                    tokens_after = event.tokens_after,
                    reason = %event.reason,
                    "CONTEXT: pre-step compaction applied"
                );
            }
            scheduler.record_checked_out_process(
                pid,
                CheckedOutProcessMetadata {
                    owner_id: process.owner_id,
                    tool_caller: process.tool_caller.clone(),
                    permission_policy: process.permission_policy.clone(),
                    state: checked_out_state_label(process.model.backend_class().as_str()),
                    checked_out_at: std::time::Instant::now(),
                    tokens: process.tokens.len(),
                    index_pos: process.index_pos,
                    max_tokens: process.max_tokens,
                    context_slot_id: process.context_slot_id,
                    resident_slot_policy: process.resident_slot_policy_label(),
                    resident_slot_state: process.resident_slot_state_label(),
                    resident_slot_snapshot_path: process
                        .resident_slot_snapshot_path()
                        .map(|path| path.display().to_string()),
                    backend_id: Some(process.model.backend_id().to_string()),
                    backend_class: Some(process.model.backend_class().as_str().to_string()),
                    backend_capabilities: Some(process.model.backend_capabilities()),
                    context: process.context_status_snapshot(),
                    pending_human_request: process.pending_human_request.clone(),
                    pending_output_buffer: process.syscall_buffer.clone(),
                    captured_assistant_text: String::new(),
                    pending_stream_syscall: None,
                },
            );
            if process.model.backend_class().as_str() == "remote_stateless" {
                audit::record(
                    storage,
                    audit::REMOTE_REQUEST_STARTED,
                    format!(
                        "pid={} backend={} awaiting=provider_response",
                        pid,
                        process.model.backend_id()
                    ),
                    AuditContext::for_process(
                        session_registry.session_id_for_pid(pid),
                        pid,
                        runtime_registry.runtime_id_for_pid(pid),
                    ),
                );
            }
            in_flight.insert(pid);
            checked_out_count = checked_out_count.saturating_add(1);
            let _ = cmd_tx.send(InferenceCmd::Step {
                pid,
                process: Box::new(process),
                eos_token_id: eos,
                eot_token_id: eot,
            });
        }
    }

    checked_out_count
}
