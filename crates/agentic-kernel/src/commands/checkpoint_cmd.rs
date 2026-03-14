use crate::checkpoint;
use crate::protocol;
use crate::scheduler::{ProcessPriority, ProcessQuota, RestoredProcessMetadata};
use crate::{audit, audit::AuditContext};
use agentic_control_models::KernelEvent;
use agentic_protocol::ControlErrorCode;

use serde_json::json;

use super::context::CheckpointCommandContext;
use super::metrics::log_event;

pub(crate) fn handle_checkpoint(ctx: CheckpointCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let CheckpointCommandContext {
        client,
        request_id,
        runtime_registry,
        model_catalog,
        scheduler,
        metrics,
        memory,
        client_id,
        ..
    } = ctx;
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    let path = if payload_text.is_empty() {
        checkpoint::default_checkpoint_path()
    } else {
        std::path::PathBuf::from(&payload_text)
    };

    let snapshot = checkpoint::build_kernel_snapshot(
        runtime_registry.current_engine(),
        model_catalog,
        scheduler,
        metrics,
        memory,
    );

    match checkpoint::save_checkpoint(&snapshot, &path) {
        Ok(msg) => {
            log_event("checkpoint_save", client_id, None, &msg);
            protocol::response_protocol_ok(
                client,
                request_id,
                "CHECKPOINT",
                protocol::schema::CHECKPOINT,
                &json!({"message": msg}),
                Some(&msg),
            )
        }
        Err(e) => protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::CheckpointFailed,
            protocol::schema::ERROR,
            &e,
        ),
    }
}

pub(crate) fn handle_restore(ctx: CheckpointCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let CheckpointCommandContext {
        client,
        request_id,
        runtime_registry,
        model_catalog,
        scheduler,
        storage,
        in_flight,
        pending_events,
        client_id,
        ..
    } = ctx;
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    let path = if payload_text.is_empty() {
        checkpoint::default_checkpoint_path()
    } else {
        std::path::PathBuf::from(&payload_text)
    };

    if !in_flight.is_empty() {
        return protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::RestoreBusy,
            protocol::schema::ERROR,
            "RESTORE requires an idle kernel: in-flight inference is still running",
        );
    }

    let live_processes = runtime_registry.live_process_count();
    if live_processes > 0 {
        return protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::RestoreBusy,
            protocol::schema::ERROR,
            &format!(
                "RESTORE requires an idle kernel: {} live process(es) still present",
                live_processes
            ),
        );
    }

    match checkpoint::load_checkpoint(&path) {
        Ok(snap) => {
            let cleared_scheduler_entries = apply_restore_snapshot(&snap, scheduler, model_catalog);
            if let Err(err) = runtime_registry.clear_loaded_runtimes(storage) {
                return protocol::response_protocol_err_typed(
                    client,
                    request_id,
                    ControlErrorCode::RestoreFailed,
                    protocol::schema::ERROR,
                    &err.to_string(),
                );
            }
            pending_events.push(KernelEvent::LobbyChanged {
                reason: "restore_applied".to_string(),
            });
            audit::record(
                storage,
                audit::KERNEL_LEGACY_RESTORE_APPLIED,
                format!(
                    "path={} restored_scheduler_entries={} processes_metadata={} semantics=legacy_metadata_only_diagnostic",
                    path.display(),
                    snap.scheduler.entries.len(),
                    snap.processes.len()
                ),
                AuditContext::default(),
            );

            let response = json!({
                "version": snap.version,
                "timestamp": snap.timestamp,
                "cleared_scheduler_entries": cleared_scheduler_entries,
                "restored_scheduler_entries": snap.scheduler.entries.len(),
                "processes_metadata": snap.processes.len(),
                "selected_model": snap.selected_model.clone().unwrap_or_default(),
                "restore_semantics": "legacy_metadata_only_diagnostic",
                "primary_persistence": "sqlite_control_plane",
                "limitations": [
                    "not_primary_persistence_path",
                    "live_processes_not_restored",
                    "model_weights_not_restored",
                    "tensor_data_not_restored",
                    "output_buffers_not_restored"
                ]
            });

            log_event(
                "checkpoint_restore",
                client_id,
                None,
                &format!(
                    "version={} cleared_sched={} restored_sched={} procs_metadata={} from={:?}",
                    snap.version,
                    cleared_scheduler_entries,
                    snap.scheduler.entries.len(),
                    snap.processes.len(),
                    path
                ),
            );
            protocol::response_protocol_ok(
                client,
                request_id,
                "RESTORE",
                protocol::schema::RESTORE,
                &response,
                Some(&response.to_string()),
            )
        }
        Err(e) => protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::RestoreFailed,
            protocol::schema::ERROR,
            &e,
        ),
    }
}

fn apply_restore_snapshot(
    snap: &checkpoint::KernelSnapshot,
    scheduler: &mut crate::scheduler::ProcessScheduler,
    model_catalog: &mut crate::model_catalog::ModelCatalog,
) -> usize {
    let existing_pids = scheduler.registered_pids();
    let cleared_scheduler_entries = existing_pids.len();
    for pid in existing_pids {
        scheduler.unregister(pid);
    }
    scheduler.clear_restored_processes();

    model_catalog.clear_selected();

    for entry in &snap.scheduler.entries {
        let priority =
            ProcessPriority::from_str_loose(&entry.priority).unwrap_or(ProcessPriority::Normal);
        let workload = crate::policy::workload_from_label_or_default(Some(&entry.workload));
        scheduler.register(entry.pid, workload, priority);
        let quota = ProcessQuota {
            max_tokens: entry.max_tokens,
            max_syscalls: entry.max_syscalls,
        };
        scheduler.set_quota(entry.pid, quota);
    }

    for process in &snap.processes {
        if scheduler.snapshot(process.pid).is_none() {
            scheduler.register(
                process.pid,
                crate::model_catalog::WorkloadClass::General,
                ProcessPriority::Normal,
            );
            scheduler.set_quota(
                process.pid,
                ProcessQuota {
                    max_tokens: process.max_tokens,
                    max_syscalls: crate::policy::scheduler_quota_defaults(
                        crate::model_catalog::WorkloadClass::General,
                    )
                    .1,
                },
            );
        }
        scheduler.record_restored_process(
            process.pid,
            RestoredProcessMetadata {
                owner_id: process.owner_id,
                state: process.state.clone(),
                token_count: process.token_count,
                max_tokens: process.max_tokens,
                context_slot_id: None,
                resident_slot_policy: None,
                resident_slot_state: None,
                resident_slot_snapshot_path: None,
                backend_id: None,
                backend_class: None,
                backend_capabilities: None,
                context_policy: process.context_policy.clone(),
                context_state: process.context_state.clone(),
            },
        );
    }

    if let Some(ref model_id) = snap.selected_model {
        let _ = model_catalog.set_selected(model_id);
    }

    cleared_scheduler_entries
}

#[cfg(test)]
#[path = "checkpoint_cmd_tests.rs"]
mod tests;

