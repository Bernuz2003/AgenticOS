use std::collections::HashSet;

use agentic_control_models::{
    ContextStatusSnapshot as ControlContextStatusSnapshot, GenerationStatus, MemoryStatus,
    ModelStatus, OrchStatusResponse, OrchSummaryResponse, OrchTaskEntry, OrchestrationsStatus,
    PidStatusResponse, ProcessesStatus, SchedulerStatus, StatusResponse,
};

use crate::commands::MetricsState;
use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::scheduler::{
    CheckedOutProcessMetadata, ProcessScheduler, ProcessSchedulerSnapshot, RestoredProcessMetadata,
};

pub struct StatusSnapshotDeps<'a> {
    pub memory: &'a NeuralMemory,
    pub engine_state: &'a Option<LLMEngine>,
    pub model_catalog: &'a ModelCatalog,
    pub scheduler: &'a ProcessScheduler,
    pub orchestrator: &'a Orchestrator,
    pub in_flight: &'a HashSet<u64>,
    pub metrics: &'a MetricsState,
}

pub fn build_global_status(deps: &StatusSnapshotDeps<'_>) -> StatusResponse {
    let (uptime_s, total_cmd, total_err, total_exec, total_signals) = deps.metrics.snapshot();
    let mem = deps.memory.snapshot();
    let (sched_tracked, sched_crit, sched_high, sched_norm, sched_low) =
        deps.scheduler.summary_counts();
    let selected_model_id = deps.model_catalog.selected_id.clone().unwrap_or_default();

    let (model_status, gen_status, processes_status) =
        if let Some(engine) = deps.engine_state.as_ref() {
            let loaded_path = engine.loaded_model_path().to_string();
            let loaded_family = engine.loaded_family();
            let loaded_model_id = deps
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
                    let in_flight_pids: Vec<u64> = deps.in_flight.iter().copied().collect();
                    let restored_pids = deps.scheduler.restored_pids();
                    let live_pids: Vec<u64> = engine.processes.keys().copied().collect();
                    let all_pids = collect_unique_pids([
                        live_pids.as_slice(),
                        active_pids.as_slice(),
                        waiting_pids.as_slice(),
                        in_flight_pids.as_slice(),
                        restored_pids.as_slice(),
                    ]);
                    let active_processes = all_pids
                        .iter()
                        .map(|&pid| build_pid_status_or_placeholder(deps, Some(engine), pid))
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
                    active_processes: deps
                        .scheduler
                        .restored_pids()
                        .into_iter()
                        .map(|pid| build_pid_status_or_placeholder(deps, None, pid))
                        .collect(),
                },
            )
        };

    StatusResponse {
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
        orchestrations: OrchestrationsStatus {
            active_orchestrations: build_orchestration_summaries(deps),
        },
        processes: processes_status,
    }
}

pub fn build_pid_status(deps: &StatusSnapshotDeps<'_>, pid: u64) -> Option<PidStatusResponse> {
    build_pid_status_response_checked(deps, deps.engine_state.as_ref(), pid)
}

pub fn build_orchestration_status(
    deps: &StatusSnapshotDeps<'_>,
    orch_id: u64,
) -> Option<OrchStatusResponse> {
    let orch = deps.orchestrator.get(orch_id)?;
    let (pending, running, completed, failed, skipped) = orch.counts();
    let total = orch.tasks.len();
    let elapsed = orch.created_at.elapsed().as_secs_f64();
    let finished = orch.is_finished();

    let tasks = orch
        .topo_order
        .iter()
        .map(|task_id| {
            let status = &orch.status[task_id];
            let (pid, error, context) = match status {
                crate::orchestrator::TaskStatus::Running { pid } => (
                    Some(*pid),
                    None,
                    build_pid_status(deps, *pid).and_then(|response| response.context),
                ),
                crate::orchestrator::TaskStatus::Failed { error } => {
                    (None, Some(error.clone()), None)
                }
                _ => (None, None, None),
            };
            OrchTaskEntry {
                task: task_id.clone(),
                status: status.label().to_string(),
                pid,
                error,
                context,
            }
        })
        .collect();

    Some(OrchStatusResponse {
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
    })
}

fn collect_unique_pids<const N: usize>(groups: [&[u64]; N]) -> Vec<u64> {
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

fn build_pid_status_or_placeholder(
    deps: &StatusSnapshotDeps<'_>,
    engine: Option<&LLMEngine>,
    pid: u64,
) -> PidStatusResponse {
    build_pid_status_response_checked(deps, engine, pid).unwrap_or_else(|| PidStatusResponse {
        pid,
        owner_id: 0,
        orchestration_id: None,
        orchestration_task_id: None,
        state: "InFlight".to_string(),
        tokens: 0,
        index_pos: 0,
        max_tokens: 0,
        priority: String::new(),
        workload: String::new(),
        quota_tokens: 0,
        quota_syscalls: 0,
        tokens_generated: 0,
        syscalls_used: 0,
        elapsed_secs: 0.0,
        context: None,
    })
}

fn build_pid_status_response_checked(
    deps: &StatusSnapshotDeps<'_>,
    engine: Option<&LLMEngine>,
    pid: u64,
) -> Option<PidStatusResponse> {
    let sched = deps.scheduler.snapshot(pid);
    let orchestration_binding = deps.orchestrator.task_binding(pid);

    if let Some(engine) = engine {
        if let Some(process) = engine.processes.get(&pid) {
            return Some(PidStatusResponse {
                pid,
                owner_id: process.owner_id,
                orchestration_id: orchestration_binding.as_ref().map(|(orch_id, _)| *orch_id),
                orchestration_task_id: orchestration_binding
                    .as_ref()
                    .map(|(_, task_id)| task_id.clone()),
                state: format!("{:?}", process.state),
                tokens: process.tokens.len(),
                index_pos: process.index_pos,
                max_tokens: process.max_tokens,
                priority: sched
                    .as_ref()
                    .map(|s| format!("{}", s.priority))
                    .unwrap_or_default(),
                workload: sched
                    .as_ref()
                    .map(|s| format!("{:?}", s.workload))
                    .unwrap_or_default(),
                quota_tokens: sched
                    .as_ref()
                    .map(|s| s.quota.max_tokens as u64)
                    .unwrap_or(0),
                quota_syscalls: sched
                    .as_ref()
                    .map(|s| s.quota.max_syscalls as u64)
                    .unwrap_or(0),
                tokens_generated: sched.as_ref().map(|s| s.tokens_generated).unwrap_or(0),
                syscalls_used: sched.as_ref().map(|s| s.syscalls_used).unwrap_or(0),
                elapsed_secs: sched.as_ref().map(|s| s.elapsed_secs).unwrap_or(0.0),
                context: Some(map_context_snapshot(process.context_status_snapshot())),
            });
        }
    }

    if let Some(checked_out) = deps.scheduler.checked_out_process(pid) {
        return Some(checked_out_pid_status_response(
            pid,
            sched.as_ref(),
            orchestration_binding,
            checked_out,
        ));
    }

    deps.scheduler
        .restored_process(pid)
        .map(|metadata| restored_pid_status_response(pid, sched.as_ref(), metadata))
}

fn checked_out_pid_status_response(
    pid: u64,
    sched: Option<&ProcessSchedulerSnapshot>,
    orchestration_binding: Option<(u64, String)>,
    metadata: &CheckedOutProcessMetadata,
) -> PidStatusResponse {
    PidStatusResponse {
        pid,
        owner_id: metadata.owner_id,
        orchestration_id: orchestration_binding.as_ref().map(|(orch_id, _)| *orch_id),
        orchestration_task_id: orchestration_binding
            .as_ref()
            .map(|(_, task_id)| task_id.clone()),
        state: metadata.state.clone(),
        tokens: metadata.tokens,
        index_pos: metadata.index_pos,
        max_tokens: metadata.max_tokens,
        priority: sched.map(|s| format!("{}", s.priority)).unwrap_or_default(),
        workload: sched
            .map(|s| format!("{:?}", s.workload))
            .unwrap_or_default(),
        quota_tokens: sched.map(|s| s.quota.max_tokens as u64).unwrap_or(0),
        quota_syscalls: sched.map(|s| s.quota.max_syscalls as u64).unwrap_or(0),
        tokens_generated: sched.map(|s| s.tokens_generated).unwrap_or(0),
        syscalls_used: sched.map(|s| s.syscalls_used).unwrap_or(0),
        elapsed_secs: sched.map(|s| s.elapsed_secs).unwrap_or(0.0),
        context: Some(map_context_snapshot(metadata.context.clone())),
    }
}

fn restored_pid_status_response(
    pid: u64,
    sched: Option<&ProcessSchedulerSnapshot>,
    metadata: &RestoredProcessMetadata,
) -> PidStatusResponse {
    PidStatusResponse {
        pid,
        owner_id: metadata.owner_id,
        orchestration_id: None,
        orchestration_task_id: None,
        state: metadata.state.clone(),
        tokens: metadata.token_count,
        index_pos: 0,
        max_tokens: metadata.max_tokens,
        priority: sched.map(|s| format!("{}", s.priority)).unwrap_or_default(),
        workload: sched
            .map(|s| format!("{:?}", s.workload))
            .unwrap_or_default(),
        quota_tokens: sched.map(|s| s.quota.max_tokens as u64).unwrap_or(0),
        quota_syscalls: sched.map(|s| s.quota.max_syscalls as u64).unwrap_or(0),
        tokens_generated: sched.map(|s| s.tokens_generated).unwrap_or(0),
        syscalls_used: sched.map(|s| s.syscalls_used).unwrap_or(0),
        elapsed_secs: sched.map(|s| s.elapsed_secs).unwrap_or(0.0),
        context: Some(map_context_snapshot(
            crate::process::ContextStatusSnapshot::from_parts(
                &metadata.context_policy,
                &metadata.context_state,
            ),
        )),
    }
}

fn map_context_snapshot(
    snapshot: crate::process::ContextStatusSnapshot,
) -> ControlContextStatusSnapshot {
    ControlContextStatusSnapshot {
        context_strategy: snapshot.context_strategy,
        context_tokens_used: snapshot.context_tokens_used,
        context_window_size: snapshot.context_window_size,
        context_compressions: snapshot.context_compressions,
        context_retrieval_hits: snapshot.context_retrieval_hits,
        last_compaction_reason: snapshot.last_compaction_reason,
        last_summary_ts: snapshot.last_summary_ts,
        context_segments: snapshot.context_segments,
    }
}

fn build_orchestration_summaries(deps: &StatusSnapshotDeps<'_>) -> Vec<OrchSummaryResponse> {
    deps.orchestrator
        .active_ids()
        .into_iter()
        .filter_map(|orch_id| {
            let orch = deps.orchestrator.get(orch_id)?;
            let (pending, running, completed, failed, skipped) = orch.counts();
            Some(OrchSummaryResponse {
                orchestration_id: orch_id,
                total: orch.tasks.len(),
                completed,
                running,
                pending,
                failed,
                skipped,
                finished: orch.is_finished(),
                elapsed_secs: orch.created_at.elapsed().as_secs_f64(),
                policy: format!("{:?}", orch.failure_policy),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::collect_unique_pids;

    #[test]
    fn collect_unique_pids_preserves_first_seen_order() {
        let unique = collect_unique_pids([&[1, 2, 3], &[3, 4], &[2, 5], &[]]);
        assert_eq!(unique, vec![1, 2, 3, 4, 5]);
    }
}
