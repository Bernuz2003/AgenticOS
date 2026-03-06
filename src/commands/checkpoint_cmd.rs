use crate::checkpoint;
use crate::protocol;
use crate::scheduler::{ProcessPriority, ProcessQuota};

use super::context::CommandContext;
use super::metrics::{log_event, snapshot_metrics};

pub(crate) fn handle_checkpoint(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    let path = if payload_text.is_empty() {
        checkpoint::default_checkpoint_path()
    } else {
        std::path::PathBuf::from(&payload_text)
    };

    let (uptime_s, total_cmd, total_err, total_exec, total_signals) = snapshot_metrics();

    let (processes, generation, active_fam, sel_model) = {
        let lock = ctx.engine_state.lock().expect("engine_state lock poisoned");
        if let Some(engine) = lock.as_ref() {
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
            (procs, gen, format!("{:?}", *ctx.active_family), ctx.model_catalog.selected_id.clone())
        } else {
            (vec![], None, format!("{:?}", *ctx.active_family), ctx.model_catalog.selected_id.clone())
        }
    };

    let sched_snap = checkpoint::snapshot_scheduler(ctx.scheduler);
    let mem_snap = checkpoint::snapshot_memory(&ctx.memory.borrow());

    let snapshot = checkpoint::KernelSnapshot {
        timestamp: checkpoint::now_timestamp(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        active_family: active_fam,
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

    match checkpoint::load_checkpoint(&path) {
        Ok(snap) => {
            for entry in &snap.scheduler.entries {
                let priority = ProcessPriority::from_str_loose(&entry.priority)
                    .unwrap_or(ProcessPriority::Normal);
                let workload = match entry.workload.to_lowercase().as_str() {
                    "fast" => crate::model_catalog::WorkloadClass::Fast,
                    "code" => crate::model_catalog::WorkloadClass::Code,
                    "reasoning" => crate::model_catalog::WorkloadClass::Reasoning,
                    _ => crate::model_catalog::WorkloadClass::General,
                };
                ctx.scheduler.register(entry.pid, workload, priority);
                let quota = ProcessQuota {
                    max_tokens: entry.max_tokens,
                    max_syscalls: entry.max_syscalls,
                };
                ctx.scheduler.set_quota(entry.pid, quota);
            }

            if let Some(ref model_id) = snap.selected_model {
                let _ = ctx.model_catalog.set_selected(model_id);
            }

            log_event(
                "checkpoint_restore",
                ctx.client_id,
                None,
                &format!(
                    "version={} procs={} sched_entries={} from={:?}",
                    snap.version,
                    snap.processes.len(),
                    snap.scheduler.entries.len(),
                    path
                ),
            );
            protocol::response_ok_code(
                "RESTORE",
                &format!(
                    "restored checkpoint version={} timestamp={} scheduler_entries={} processes_metadata={}",
                    snap.version, snap.timestamp, snap.scheduler.entries.len(), snap.processes.len()
                ),
            )
        }
        Err(e) => protocol::response_err_code("RESTORE_FAILED", &e),
    }
}
