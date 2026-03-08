use crate::checkpoint;
use crate::protocol;
use crate::scheduler::{ProcessPriority, ProcessQuota};

use serde_json::json;

use super::context::CommandContext;
use super::metrics::log_event;

fn current_family_snapshot(ctx: &CommandContext<'_>) -> String {
    ctx.engine_state
        .as_ref()
        .map(|engine| format!("{:?}", engine.loaded_family()))
        .or_else(|| {
            ctx.model_catalog
                .selected_entry()
                .map(|entry| format!("{:?}", entry.family))
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

pub(crate) fn handle_checkpoint(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    let path = if payload_text.is_empty() {
        checkpoint::default_checkpoint_path()
    } else {
        std::path::PathBuf::from(&payload_text)
    };

    let (uptime_s, total_cmd, total_err, total_exec, total_signals) = ctx.metrics.snapshot();

    let (processes, generation, sel_model) = {
        if let Some(engine) = ctx.engine_state.as_ref() {
            let procs: Vec<checkpoint::ProcessSnapshot> = engine
                .processes
                .iter()
                .map(|(pid, p)| checkpoint::ProcessSnapshot {
                    pid: *pid,
                    owner_id: p.owner_id,
                    state: format!("{:?}", p.state),
                    token_count: p.tokens.len(),
                    max_tokens: p.max_tokens,
                })
                .collect();
            let cfg = engine.generation_config();
            let gen = Some(checkpoint::GenerationSnapshot {
                temperature: cfg.temperature,
                top_p: cfg.top_p,
                seed: cfg.seed,
                max_tokens: cfg.max_tokens,
            });
            (procs, gen, ctx.model_catalog.selected_id.clone())
        } else {
            (vec![], None, ctx.model_catalog.selected_id.clone())
        }
    };

    let sched_snap = checkpoint::snapshot_scheduler(ctx.scheduler);
    let mem_snap = checkpoint::snapshot_memory(ctx.memory);

    let snapshot = checkpoint::KernelSnapshot {
        timestamp: checkpoint::now_timestamp(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        active_family: current_family_snapshot(ctx),
        selected_model: sel_model,
        generation,
        processes,
        scheduler: sched_snap,
        metrics: checkpoint::MetricsSnapshot {
            uptime_secs: uptime_s,
            total_commands: total_cmd,
            total_errors: total_err,
            total_exec_started: total_exec,
            total_signals,
        },
        memory: mem_snap,
    };

    match checkpoint::save_checkpoint(&snapshot, &path) {
        Ok(msg) => {
            log_event("checkpoint_save", ctx.client_id, None, &msg);
            protocol::response_ok_code("CHECKPOINT", &msg)
        }
        Err(e) => protocol::response_err_code("CHECKPOINT_FAILED", &e),
    }
}

pub(crate) fn handle_restore(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    let path = if payload_text.is_empty() {
        checkpoint::default_checkpoint_path()
    } else {
        std::path::PathBuf::from(&payload_text)
    };

    if !ctx.in_flight.is_empty() {
        return protocol::response_err_code(
            "RESTORE_BUSY",
            "RESTORE requires an idle kernel: in-flight inference is still running",
        );
    }

    if let Some(engine) = ctx.engine_state.as_ref() {
        let live_processes = engine.processes.len();
        if live_processes > 0 {
            return protocol::response_err_code(
                "RESTORE_BUSY",
                &format!(
                    "RESTORE requires an idle kernel: {} live process(es) still present",
                    live_processes
                ),
            );
        }
    }

    match checkpoint::load_checkpoint(&path) {
        Ok(snap) => {
            let cleared_scheduler_entries =
                apply_restore_snapshot(&snap, ctx.scheduler, ctx.model_catalog);
            *ctx.engine_state = None;

            let response = json!({
                "version": snap.version,
                "timestamp": snap.timestamp,
                "cleared_scheduler_entries": cleared_scheduler_entries,
                "restored_scheduler_entries": snap.scheduler.entries.len(),
                "processes_metadata": snap.processes.len(),
                "selected_model": snap.selected_model.clone().unwrap_or_default(),
                "restore_semantics": "metadata_only_clear_and_apply",
                "limitations": [
                    "live_processes_not_restored",
                    "model_weights_not_restored",
                    "tensor_data_not_restored",
                    "output_buffers_not_restored"
                ]
            });

            log_event(
                "checkpoint_restore",
                ctx.client_id,
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
            protocol::response_ok_code("RESTORE", &response.to_string())
        }
        Err(e) => protocol::response_err_code("RESTORE_FAILED", &e),
    }
}

fn workload_from_snapshot(raw: &str) -> crate::model_catalog::WorkloadClass {
    match raw.to_lowercase().as_str() {
        "fast" => crate::model_catalog::WorkloadClass::Fast,
        "code" => crate::model_catalog::WorkloadClass::Code,
        "reasoning" => crate::model_catalog::WorkloadClass::Reasoning,
        _ => crate::model_catalog::WorkloadClass::General,
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

    model_catalog.selected_id = None;

    for entry in &snap.scheduler.entries {
        let priority = ProcessPriority::from_str_loose(&entry.priority)
            .unwrap_or(ProcessPriority::Normal);
        let workload = workload_from_snapshot(&entry.workload);
        scheduler.register(entry.pid, workload, priority);
        let quota = ProcessQuota {
            max_tokens: entry.max_tokens,
            max_syscalls: entry.max_syscalls,
        };
        scheduler.set_quota(entry.pid, quota);
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
        scheduler.register(1, crate::model_catalog::WorkloadClass::Fast, ProcessPriority::High);
        scheduler.register(2, crate::model_catalog::WorkloadClass::General, ProcessPriority::Low);

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
