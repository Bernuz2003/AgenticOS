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
mod tests {
    use super::*;
    use crate::checkpoint::{
        GenerationSnapshot, KernelSnapshot, MemoryCountersSnapshot, MetricsSnapshot,
        ProcessSnapshot, SchedulerEntrySnapshot, SchedulerStateSnapshot,
    };
    use crate::model_catalog::ModelCatalog;
    use crate::process::{ContextPolicy, ContextState, ContextStrategy};
    use crate::scheduler::ProcessScheduler;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn apply_restore_snapshot_clears_existing_scheduler_state() {
        let base = mk_temp_dir("agenticos_restore_apply");
        let models = base.join("models");
        let llama_dir = models.join("llama3.1-8b");
        fs::create_dir_all(&llama_dir).expect("create llama dir");
        fs::write(llama_dir.join("meta-llama-3.1-8b.gguf"), b"stub").expect("write model");

        let mut catalog = ModelCatalog::discover(&models).expect("discover models");
        let model_id = catalog.entries[0].id.clone();
        let mut scheduler = ProcessScheduler::new();
        scheduler.register(
            1,
            crate::model_catalog::WorkloadClass::Fast,
            ProcessPriority::High,
        );
        scheduler.register(
            2,
            crate::model_catalog::WorkloadClass::General,
            ProcessPriority::Low,
        );

        let snapshot = KernelSnapshot {
            timestamp: "epoch_1".to_string(),
            version: "0.5.0".to_string(),
            active_family: "Qwen".to_string(),
            selected_model: Some(model_id.clone()),
            generation: Some(GenerationSnapshot {
                temperature: 0.7,
                top_p: 0.9,
                seed: 42,
                max_tokens: 256,
            }),
            processes: vec![ProcessSnapshot {
                pid: 7,
                owner_id: 1,
                state: "Orphaned".to_string(),
                token_count: 0,
                max_tokens: 256,
                context_policy: ContextPolicy::new(ContextStrategy::Summarize, 256, 224, 192, 3),
                context_state: ContextState {
                    tokens_used: 12,
                    context_compressions: 1,
                    context_retrieval_hits: 0,
                    last_compaction_reason: Some(
                        "summarize_compacted_segments=2 replaced_tokens=42".to_string(),
                    ),
                    last_summary_ts: Some("epoch_123".to_string()),
                    segments: Vec::new(),
                    episodic_segments: Vec::new(),
                },
            }],
            scheduler: SchedulerStateSnapshot {
                entries: vec![SchedulerEntrySnapshot {
                    pid: 7,
                    priority: "critical".to_string(),
                    workload: "code".to_string(),
                    max_tokens: 1024,
                    max_syscalls: 8,
                    tokens_generated: 0,
                    syscalls_used: 0,
                    elapsed_secs: 0.0,
                }],
            },
            metrics: MetricsSnapshot {
                uptime_secs: 1,
                total_commands: 1,
                total_errors: 0,
                total_exec_started: 0,
                total_signals: 0,
            },
            memory: MemoryCountersSnapshot {
                active: false,
                total_blocks: 0,
                free_blocks: 0,
                allocated_tensors: 0,
                tracked_pids: 0,
                alloc_bytes: 0,
                evictions: 0,
                swap_count: 0,
                swap_faults: 0,
                oom_events: 0,
            },
        };

        let cleared = apply_restore_snapshot(&snapshot, &mut scheduler, &mut catalog);
        assert_eq!(cleared, 2);
        assert_eq!(scheduler.registered_pids(), vec![7]);
        assert_eq!(catalog.selected_id.as_deref(), Some(model_id.as_str()));
        let restored = scheduler
            .restored_process(7)
            .expect("restored process exists");
        assert_eq!(restored.context_policy.strategy, ContextStrategy::Summarize);
        assert_eq!(restored.context_state.tokens_used, 12);

        let _ = fs::remove_dir_all(base);
    }

    fn mk_temp_dir(prefix: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time ok")
            .as_nanos();
        std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), ts))
    }
}
