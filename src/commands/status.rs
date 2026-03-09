use serde::Serialize;
use std::collections::HashSet;

use crate::protocol;

use super::context::CommandContext;

// ── JSON response types ─────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct StatusResponse {
    pub uptime_secs: u64,
    pub total_commands: u64,
    pub total_errors: u64,
    pub total_exec_started: u64,
    pub total_signals: u64,
    pub model: ModelStatus,
    pub generation: Option<GenerationStatus>,
    pub memory: MemoryStatus,
    pub scheduler: SchedulerStatus,
    pub processes: ProcessesStatus,
}

#[derive(Serialize)]
pub(crate) struct ModelStatus {
    pub loaded: bool,
    pub loaded_model_id: String,
    pub loaded_family: String,
    pub loaded_model_path: String,
    pub selected_model_id: String,
}

#[derive(Serialize)]
pub(crate) struct GenerationStatus {
    pub temperature: f64,
    pub top_p: f64,
    pub seed: u64,
    pub max_tokens: usize,
}

#[derive(Serialize)]
pub(crate) struct MemoryStatus {
    pub active: bool,
    pub total_blocks: usize,
    pub free_blocks: usize,
    pub tracked_pids: usize,
    pub allocated_tensors: usize,
    pub alloc_bytes: usize,
    pub evictions: u64,
    pub swap_count: u64,
    pub swap_faults: u64,
    pub swap_failures: u64,
    pub pending_swaps: usize,
    pub waiting_pids: usize,
    pub oom_events: u64,
    pub swap_worker_crashes: u64,
}

#[derive(Serialize)]
pub(crate) struct SchedulerStatus {
    pub tracked: usize,
    pub priority_critical: usize,
    pub priority_high: usize,
    pub priority_normal: usize,
    pub priority_low: usize,
}

#[derive(Serialize)]
pub(crate) struct ProcessesStatus {
    pub active_pids: Vec<u64>,
    pub waiting_pids: Vec<u64>,
    pub in_flight_pids: Vec<u64>,
    pub active_processes: Vec<PidStatusResponse>,
}

#[derive(Serialize)]
pub(crate) struct PidStatusResponse {
    pub pid: u64,
    pub owner_id: usize,
    pub state: String,
    pub tokens: usize,
    pub index_pos: usize,
    pub max_tokens: usize,
    pub priority: String,
    pub workload: String,
    pub quota_tokens: u64,
    pub quota_syscalls: u64,
    pub tokens_generated: usize,
    pub syscalls_used: usize,
    pub elapsed_secs: f64,
}

#[derive(Serialize)]
pub(crate) struct OrchStatusResponse {
    pub orchestration_id: u64,
    pub total: usize,
    pub completed: usize,
    pub running: usize,
    pub pending: usize,
    pub failed: usize,
    pub skipped: usize,
    pub finished: bool,
    pub elapsed_secs: f64,
    pub policy: String,
    pub truncations: usize,
    pub output_chars_stored: usize,
    pub tasks: Vec<OrchTaskEntry>,
}

#[derive(Serialize)]
pub(crate) struct OrchTaskEntry {
    pub task: String,
    pub status: String,
    pub pid: Option<u64>,
    pub error: Option<String>,
}

// ── Handler ─────────────────────────────────────────────────────────────

pub(crate) fn handle_status(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let requested = String::from_utf8_lossy(payload).trim().to_string();
    let (uptime_s, total_cmd, total_err, total_exec, total_signals) = ctx.metrics.snapshot();

    // ── Orchestration status ───────────────────────────────────────
    if let Some(orch_id_str) = requested.strip_prefix("orch:") {
        return match orch_id_str.parse::<u64>() {
            Ok(orch_id) => build_orch_status(ctx, orch_id),
            Err(_) => protocol::response_err_code(
                "STATUS_INVALID",
                "Orchestration ID must be numeric (orch:<N>)",
            ),
        };
    }

    // ── Per-PID status ─────────────────────────────────────────────
    if !requested.is_empty() {
        return match requested.parse::<u64>() {
            Ok(pid) => build_pid_status(ctx, pid),
            Err(_) => protocol::response_err_code(
                "STATUS_INVALID",
                "STATUS payload must be empty, numeric PID, or orch:<N>",
            ),
        };
    }

    // ── Global status (JSON) ───────────────────────────────────────
    let mem = ctx.memory.snapshot();
    let (sched_tracked, sched_crit, sched_high, sched_norm, sched_low) =
        ctx.scheduler.summary_counts();
    let selected_model_id = ctx
        .model_catalog
        .selected_id
        .clone()
        .unwrap_or_default();

    let (model_status, gen_status, processes_status) =
        if let Some(engine) = ctx.engine_state.as_ref() {
            let loaded_path = engine.loaded_model_path().to_string();
            let loaded_family = engine.loaded_family();
            let loaded_model_id = ctx
                .model_catalog
                .entries
                .iter()
                .find(|entry| entry.path.to_string_lossy() == loaded_path)
                .map(|entry| entry.id.clone())
                .unwrap_or_default();
            let cfg = engine.generation_config();
            (
                ModelStatus {
                    loaded: true,
                    loaded_model_id,
                    loaded_family: format!("{:?}", loaded_family),
                    loaded_model_path: loaded_path,
                    selected_model_id: selected_model_id.clone(),
                },
                Some(GenerationStatus {
                    temperature: cfg.temperature,
                    top_p: cfg.top_p,
                    seed: cfg.seed,
                    max_tokens: cfg.max_tokens,
                }),
                {
                    let active_pids = engine.list_active_pids();
                    let waiting_pids = engine.list_waiting_pids();
                    let in_flight_pids: Vec<u64> = ctx.in_flight.iter().copied().collect();
                    let all_pids = collect_unique_pids([
                        active_pids.as_slice(),
                        waiting_pids.as_slice(),
                        in_flight_pids.as_slice(),
                    ]);
                    let active_processes: Vec<PidStatusResponse> = all_pids
                        .iter()
                        .map(|&pid| {
                            let sched = ctx.scheduler.snapshot(pid);
                            let (owner_id, state, tokens, index_pos, max_tokens) =
                                if let Some(process) = engine.processes.get(&pid) {
                                    (
                                        process.owner_id,
                                        format!("{:?}", process.state),
                                        process.tokens.len(),
                                        process.index_pos,
                                        process.max_tokens,
                                    )
                                } else {
                                    // In-flight (checked out for inference)
                                    (0, "InFlight".to_string(), 0, 0, 0)
                                };
                            PidStatusResponse {
                                pid,
                                owner_id,
                                state,
                                tokens,
                                index_pos,
                                max_tokens,
                                priority: sched.as_ref().map(|s| format!("{}", s.priority)).unwrap_or_default(),
                                workload: sched.as_ref().map(|s| format!("{:?}", s.workload)).unwrap_or_default(),
                                quota_tokens: sched.as_ref().map(|s| s.quota.max_tokens as u64).unwrap_or(0),
                                quota_syscalls: sched.as_ref().map(|s| s.quota.max_syscalls as u64).unwrap_or(0),
                                tokens_generated: sched.as_ref().map(|s| s.tokens_generated).unwrap_or(0),
                                syscalls_used: sched.as_ref().map(|s| s.syscalls_used).unwrap_or(0),
                                elapsed_secs: sched.as_ref().map(|s| s.elapsed_secs).unwrap_or(0.0),
                            }
                        })
                        .collect();
                    ProcessesStatus {
                        active_pids,
                        waiting_pids,
                        in_flight_pids,
                        active_processes,
                    }
                },
            )
        } else {
            (
                ModelStatus {
                    loaded: false,
                    loaded_model_id: String::new(),
                    loaded_family: "Unknown".to_string(),
                    loaded_model_path: String::new(),
                    selected_model_id: selected_model_id.clone(),
                },
                None,
                ProcessesStatus {
                    active_pids: vec![],
                    waiting_pids: vec![],
                    in_flight_pids: vec![],
                    active_processes: vec![],
                },
            )
        };

    let resp = StatusResponse {
        uptime_secs: uptime_s,
        total_commands: total_cmd,
        total_errors: total_err,
        total_exec_started: total_exec,
        total_signals,
        model: model_status,
        generation: gen_status,
        memory: MemoryStatus {
            active: mem.active,
            total_blocks: mem.total_blocks,
            free_blocks: mem.free_blocks,
            tracked_pids: mem.tracked_pids,
            allocated_tensors: mem.allocated_tensors,
            alloc_bytes: mem.alloc_bytes,
            evictions: mem.evictions,
            swap_count: mem.swap_count,
            swap_faults: mem.swap_faults,
            swap_failures: mem.swap_failures,
            pending_swaps: mem.pending_swaps,
            waiting_pids: mem.waiting_pids,
            oom_events: mem.oom_events,
            swap_worker_crashes: mem.swap_worker_crashes,
        },
        scheduler: SchedulerStatus {
            tracked: sched_tracked,
            priority_critical: sched_crit,
            priority_high: sched_high,
            priority_normal: sched_norm,
            priority_low: sched_low,
        },
        processes: processes_status,
    };

    let json = serde_json::to_string(&resp).expect("StatusResponse is always serializable");
    protocol::response_protocol_ok(
        ctx.client,
        &ctx.request_id,
        "STATUS",
        protocol::schema::STATUS,
        &resp,
        Some(&json),
    )
}

fn collect_unique_pids(groups: [&[u64]; 3]) -> Vec<u64> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();

    for group in groups {
        for &pid in group {
            if seen.insert(pid) {
                unique.push(pid);
            }
        }
    }

    unique
}

// ── Per-PID status (JSON) ───────────────────────────────────────────────

fn build_pid_status(ctx: &mut CommandContext<'_>, pid: u64) -> Vec<u8> {
    let engine = match ctx.engine_state.as_ref() {
        Some(e) => e,
        None => {
            return protocol::response_err_code("NO_ENGINE", "No model loaded");
        }
    };

    let process = match engine.processes.get(&pid) {
        Some(p) => p,
        None => {
            return protocol::response_err_code(
                "PID_NOT_FOUND",
                &format!("PID {} not found", pid),
            );
        }
    };

    let sched = ctx.scheduler.snapshot(pid);
    let resp = PidStatusResponse {
        pid,
        owner_id: process.owner_id,
        state: format!("{:?}", process.state),
        tokens: process.tokens.len(),
        index_pos: process.index_pos,
        max_tokens: process.max_tokens,
        priority: sched.as_ref().map(|s| format!("{}", s.priority)).unwrap_or_default(),
        workload: sched.as_ref().map(|s| format!("{:?}", s.workload)).unwrap_or_default(),
        quota_tokens: sched.as_ref().map(|s| s.quota.max_tokens as u64).unwrap_or(0),
        quota_syscalls: sched.as_ref().map(|s| s.quota.max_syscalls as u64).unwrap_or(0),
        tokens_generated: sched.as_ref().map(|s| s.tokens_generated).unwrap_or(0),
        syscalls_used: sched.as_ref().map(|s| s.syscalls_used).unwrap_or(0),
        elapsed_secs: sched.as_ref().map(|s| s.elapsed_secs).unwrap_or(0.0),
    };

    let json = serde_json::to_string(&resp).expect("PidStatusResponse is always serializable");
    protocol::response_protocol_ok(
        ctx.client,
        &ctx.request_id,
        "STATUS",
        protocol::schema::PID_STATUS,
        &resp,
        Some(&json),
    )
}

// ── Orchestration status (JSON) ─────────────────────────────────────────

fn build_orch_status(ctx: &mut CommandContext<'_>, orch_id: u64) -> Vec<u8> {
    let orch = match ctx.orchestrator.get(orch_id) {
        Some(o) => o,
        None => {
            return protocol::response_err_code(
                "ORCH_NOT_FOUND",
                &format!("Orchestration {} not found", orch_id),
            );
        }
    };

    let (pending, running, completed, failed, skipped) = orch.counts();
    let total = orch.tasks.len();
    let elapsed = orch.created_at.elapsed().as_secs_f64();
    let finished = orch.is_finished();

    let tasks: Vec<OrchTaskEntry> = orch
        .topo_order
        .iter()
        .map(|task_id| {
            let status = &orch.status[task_id];
            let (pid, error) = match status {
                crate::orchestrator::TaskStatus::Running { pid } => (Some(*pid), None),
                crate::orchestrator::TaskStatus::Failed { error } => {
                    (None, Some(error.clone()))
                }
                _ => (None, None),
            };
            OrchTaskEntry {
                task: task_id.clone(),
                status: status.label().to_string(),
                pid,
                error,
            }
        })
        .collect();

    let resp = OrchStatusResponse {
        orchestration_id: orch_id,
        total,
        completed,
        running,
        pending,
        failed,
        skipped,
        finished,
        elapsed_secs: elapsed,
        policy: format!("{:?}", orch.failure_policy),
        truncations: orch.truncated_outputs,
        output_chars_stored: orch.output_chars_stored,
        tasks,
    };

    let json = serde_json::to_string(&resp).expect("OrchStatusResponse is always serializable");
    protocol::response_protocol_ok(
        ctx.client,
        &ctx.request_id,
        "STATUS",
        protocol::schema::ORCH_STATUS,
        &resp,
        Some(&json),
    )
}

#[cfg(test)]
mod tests {
    use super::collect_unique_pids;

    #[test]
    fn collect_unique_pids_preserves_first_seen_order() {
        let unique = collect_unique_pids([&[1, 2, 3], &[3, 4], &[2, 5]]);
        assert_eq!(unique, vec![1, 2, 3, 4, 5]);
    }
}
