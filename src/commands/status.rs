use crate::protocol;

use super::context::CommandContext;
use super::metrics::snapshot_metrics;

pub(crate) fn handle_status(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let requested = String::from_utf8_lossy(payload).trim().to_string();
    let lock = ctx.engine_state.lock().expect("engine_state lock poisoned");
    let (uptime_s, total_cmd, total_err, total_exec, total_signals) = snapshot_metrics();

    if let Some(engine) = lock.as_ref() {
        if requested.is_empty() {
            let active = engine.list_active_pids();
            let waiting = engine.list_waiting_pids();
            let cfg = engine.generation_config();
            let mem = ctx.memory.borrow().snapshot();
            let loaded_path = engine.loaded_model_path().to_string();
            let loaded_family = engine.loaded_family();
            let loaded_model_id = ctx
                .model_catalog
                .entries
                .iter()
                .find(|entry| entry.path.to_string_lossy() == loaded_path)
                .map(|entry| entry.id.clone())
                .unwrap_or_else(|| "<unknown>".to_string());
            let selected_model_id = ctx
                .model_catalog
                .selected_id
                .clone()
                .unwrap_or_else(|| "<none>".to_string());
            let sched_summary = ctx.scheduler.summary();
            protocol::response_ok_code(
                "STATUS",
                &format!(
                    "uptime_s={} total_commands={} total_errors={} total_exec_started={} total_signals={} active_processes={} waiting_processes={} active_pids={:?} waiting_pids={:?} selected_model_id={} loaded_model_id={} loaded_family={:?} loaded_model_path={} generation=temperature:{} top_p:{} seed:{} max_tokens:{} mem_active={} mem_total_blocks={} mem_free_blocks={} mem_tracked_pids={} mem_allocated_tensors={} mem_alloc_bytes={} mem_evictions={} mem_swap_count={} mem_swap_faults={} mem_swap_failures={} mem_pending_swaps={} mem_waiting_pids={} mem_oom_events={} {}",
                    uptime_s,
                    total_cmd,
                    total_err,
                    total_exec,
                    total_signals,
                    active.len(),
                    waiting.len(),
                    active,
                    waiting,
                    selected_model_id,
                    loaded_model_id,
                    loaded_family,
                    loaded_path,
                    cfg.temperature,
                    cfg.top_p,
                    cfg.seed,
                    cfg.max_tokens,
                    mem.active,
                    mem.total_blocks,
                    mem.free_blocks,
                    mem.tracked_pids,
                    mem.allocated_tensors,
                    mem.alloc_bytes,
                    mem.evictions,
                    mem.swap_count,
                    mem.swap_faults,
                    mem.swap_failures,
                    mem.pending_swaps,
                    mem.waiting_pids,
                    mem.oom_events,
                    sched_summary
                ),
            )
        } else if let Some(orch_id_str) = requested.strip_prefix("orch:") {
            if let Ok(orch_id) = orch_id_str.parse::<u64>() {
                if let Some(status_text) = ctx.orchestrator.format_status(orch_id) {
                    protocol::response_ok_code("STATUS", &status_text)
                } else {
                    protocol::response_err_code(
                        "ORCH_NOT_FOUND",
                        &format!("Orchestration {} not found", orch_id),
                    )
                }
            } else {
                protocol::response_err_code(
                    "STATUS_INVALID",
                    "Orchestration ID must be numeric (orch:<N>)",
                )
            }
        } else if let Ok(pid) = requested.parse::<u64>() {
            if let Some(line) = engine.process_status_line(pid) {
                let sched_info = ctx.scheduler.snapshot(pid).map(|s| {
                    format!(
                        " priority={} workload={:?} quota_tokens={} quota_syscalls={} tokens_generated={} syscalls_used={} elapsed_secs={:.2}",
                        s.priority, s.workload, s.quota.max_tokens, s.quota.max_syscalls,
                        s.tokens_generated, s.syscalls_used, s.elapsed_secs
                    )
                }).unwrap_or_default();
                protocol::response_ok_code("STATUS", &format!("{}{}", line, sched_info))
            } else {
                protocol::response_err_code(
                    "PID_NOT_FOUND",
                    &format!("PID {} not found", pid),
                )
            }
        } else {
            protocol::response_err_code(
                "STATUS_INVALID",
                "STATUS payload must be empty or numeric PID",
            )
        }
    } else {
        protocol::response_ok_code(
            "STATUS",
            &format!(
                "uptime_s={} total_commands={} total_errors={} total_exec_started={} total_signals={} active_processes=0 active_pids=[] selected_model_id={} loaded_model_id=<none> loaded_family=Unknown loaded_model_path=<none> no_model_loaded=true",
                uptime_s, total_cmd, total_err, total_exec, total_signals,
                ctx.model_catalog.selected_id.clone().unwrap_or_else(|| "<none>".to_string())
            ),
        )
    }
}
