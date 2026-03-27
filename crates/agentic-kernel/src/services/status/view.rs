use std::collections::HashSet;

use agentic_control_models::{
    GenerationStatus, JobsStatus, MemoryStatus, ModelStatus, OrchestrationsStatus,
    ProcessesStatus, ScheduledJobListResponse, SchedulerStatus, StatusResponse,
};

use crate::backend::runtime_backend_telemetry;
use crate::commands::MetricsState;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::resource_governor::ResourceGovernor;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::services::job_scheduler::JobScheduler;
use crate::session::SessionRegistry;
use crate::storage::StorageService;

use super::orchestration::build_orchestration_summaries;
use super::process::build_pid_status_or_placeholder;
use super::resources::{
    build_resource_governor_view, build_runtime_load_queue, collect_unique_pids,
};
use super::runtime::{
    build_managed_local_runtimes, build_runtime_instances, current_loaded_target_info,
};

pub use super::orchestration::{
    build_artifact_list, build_orchestration_list, build_orchestration_status,
};
#[allow(unused_imports)]
pub(crate) use super::process::{
    build_pid_status, checked_out_pid_status_response, restored_pid_status_response,
};
#[allow(unused_imports)]
pub(crate) use super::runtime::runtime_backend_status;

pub struct StatusSnapshotDeps<'a> {
    pub memory: &'a NeuralMemory,
    pub runtime_registry: &'a RuntimeRegistry,
    pub resource_governor: &'a ResourceGovernor,
    pub model_catalog: &'a ModelCatalog,
    pub scheduler: &'a ProcessScheduler,
    pub job_scheduler: &'a JobScheduler,
    pub orchestrator: &'a Orchestrator,
    pub in_flight: &'a HashSet<u64>,
    pub metrics: &'a MetricsState,
    pub session_registry: &'a SessionRegistry,
    pub storage: &'a StorageService,
}

pub fn build_global_status(deps: &StatusSnapshotDeps<'_>) -> StatusResponse {
    let (uptime_s, total_cmd, total_err, total_exec, total_signals) = deps.metrics.snapshot();
    let mem = deps.memory.snapshot();
    let (sched_tracked, sched_crit, sched_high, sched_norm, sched_low) =
        deps.scheduler.summary_counts();
    let selected_model_id = deps.model_catalog.selected_id.clone().unwrap_or_default();
    let runtime_instances = build_runtime_instances(deps);
    let governor_view = build_resource_governor_view(deps);
    let runtime_load_queue = build_runtime_load_queue(deps);
    let managed_local_runtimes = build_managed_local_runtimes();
    let global_accounting = deps
        .storage
        .global_accounting_summary()
        .ok()
        .flatten()
        .map(|summary| summary.into_view());

    let (model_status, gen_status, processes_status) =
        if let Some(engine) = deps.runtime_registry.current_engine() {
            let loaded_path = engine.loaded_model_path().to_string();
            let loaded_family = engine.loaded_family();
            let loaded_remote_model = engine.loaded_remote_model().cloned();
            let (loaded_model_id, loaded_target_kind, loaded_provider_id, loaded_remote_model_id) =
                current_loaded_target_info(
                    deps.model_catalog,
                    std::path::Path::new(&loaded_path),
                    loaded_remote_model.as_ref(),
                );
            let cfg = engine.generation_config();
            (
                ModelStatus {
                    loaded: true,
                    loaded_model_id,
                    loaded_family: format!("{:?}", loaded_family),
                    loaded_model_path: loaded_path,
                    selected_model_id: selected_model_id.clone(),
                    loaded_target_kind: Some(loaded_target_kind),
                    loaded_provider_id,
                    loaded_remote_model_id,
                    loaded_backend: Some(engine.backend_id.clone()),
                    loaded_backend_class: Some(
                        engine
                            .master_model
                            .as_ref()
                            .map(|model| model.backend_class().as_str().to_string())
                            .unwrap_or_else(|| "unknown".to_string()),
                    ),
                    loaded_backend_capabilities: engine
                        .master_model
                        .as_ref()
                        .map(|model| model.backend_capabilities().into()),
                    loaded_backend_telemetry: deps
                        .storage
                        .accounting_summary_for_backend(&engine.backend_id)
                        .ok()
                        .flatten()
                        .map(|summary| summary.into_view())
                        .or_else(|| runtime_backend_telemetry(&engine.backend_id)),
                    loaded_remote_model,
                    runtime_instances: runtime_instances.clone(),
                    managed_local_runtimes: managed_local_runtimes.clone(),
                    resource_governor: Some(governor_view.clone()),
                    runtime_load_queue: runtime_load_queue.clone(),
                },
                Some(GenerationStatus {
                    temperature: cfg.temperature,
                    top_p: cfg.top_p,
                    seed: cfg.seed,
                    max_tokens: cfg.max_tokens,
                }),
                {
                    let active_pids = deps
                        .runtime_registry
                        .loaded_runtime_ids()
                        .into_iter()
                        .flat_map(|runtime_id| {
                            deps.runtime_registry
                                .engine(&runtime_id)
                                .map(|engine| engine.list_active_pids())
                                .unwrap_or_default()
                        })
                        .collect::<Vec<_>>();
                    let parked_pids = deps
                        .runtime_registry
                        .loaded_runtime_ids()
                        .into_iter()
                        .flat_map(|runtime_id| {
                            deps.runtime_registry
                                .engine(&runtime_id)
                                .map(|engine| engine.list_parked_pids())
                                .unwrap_or_default()
                        })
                        .collect::<Vec<_>>();
                    let in_flight_pids: Vec<u64> = deps.in_flight.iter().copied().collect();
                    let restored_pids = deps.scheduler.restored_pids();
                    let live_pids: Vec<u64> = deps.runtime_registry.all_active_pids();
                    let all_pids = collect_unique_pids([
                        live_pids.as_slice(),
                        active_pids.as_slice(),
                        parked_pids.as_slice(),
                        in_flight_pids.as_slice(),
                        restored_pids.as_slice(),
                    ]);
                    let active_processes = all_pids
                        .iter()
                        .map(|&pid| {
                            let engine = deps
                                .runtime_registry
                                .runtime_id_for_pid(pid)
                                .and_then(|runtime_id| deps.runtime_registry.engine(runtime_id));
                            build_pid_status_or_placeholder(deps, engine, pid)
                        })
                        .collect();
                    ProcessesStatus {
                        active_pids,
                        parked_pids,
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
                    loaded_target_kind: None,
                    loaded_provider_id: None,
                    loaded_remote_model_id: None,
                    loaded_backend: None,
                    loaded_backend_class: None,
                    loaded_backend_capabilities: None,
                    loaded_backend_telemetry: None,
                    loaded_remote_model: None,
                    runtime_instances: runtime_instances.clone(),
                    managed_local_runtimes: managed_local_runtimes.clone(),
                    resource_governor: Some(governor_view.clone()),
                    runtime_load_queue: runtime_load_queue.clone(),
                },
                None,
                ProcessesStatus {
                    active_pids: vec![],
                    parked_pids: vec![],
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
        global_accounting,
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
            parked_pids: mem.parked_pids,
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
        jobs: JobsStatus {
            scheduled_jobs: deps
                .job_scheduler
                .scheduled_jobs()
                .into_iter()
                .map(|job| job.to_view())
                .collect(),
        },
        orchestrations: OrchestrationsStatus {
            active_orchestrations: build_orchestration_summaries(deps),
        },
        processes: processes_status,
    }
}

pub fn build_scheduled_job_list(deps: &StatusSnapshotDeps<'_>) -> ScheduledJobListResponse {
    ScheduledJobListResponse {
        jobs: deps
            .job_scheduler
            .scheduled_jobs()
            .into_iter()
            .map(|job| job.to_view())
            .collect(),
    }
}

#[cfg(test)]
#[path = "../tests/status_view.rs"]
mod tests;
