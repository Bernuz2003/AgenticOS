use std::collections::HashSet;

use agentic_control_models::{
    BackendCapabilitiesView, BackendTelemetryView,
    ContextStatusSnapshot as ControlContextStatusSnapshot, GenerationStatus, MemoryStatus,
    ModelStatus, OrchStatusResponse, OrchSummaryResponse, OrchTaskEntry, OrchestrationsStatus,
    PidStatusResponse, ProcessesStatus, ResourceGovernorStatusView, RuntimeInstanceView,
    RuntimeLoadQueueEntryView, SchedulerStatus, StatusResponse,
};

use crate::backend::{runtime_backend_telemetry, BackendCapabilities, RuntimeModel};
use crate::commands::MetricsState;
use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::resource_governor::ResourceGovernor;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::{
    CheckedOutProcessMetadata, ProcessScheduler, ProcessSchedulerSnapshot, RestoredProcessMetadata,
};
use crate::session::SessionRegistry;
use crate::storage::StorageService;

pub struct StatusSnapshotDeps<'a> {
    pub memory: &'a NeuralMemory,
    pub runtime_registry: &'a RuntimeRegistry,
    pub resource_governor: &'a ResourceGovernor,
    pub model_catalog: &'a ModelCatalog,
    pub scheduler: &'a ProcessScheduler,
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
    let runtime_instances: Vec<RuntimeInstanceView> = deps
        .runtime_registry
        .runtime_views()
        .into_iter()
        .map(|runtime| RuntimeInstanceView {
            runtime_id: runtime.runtime_id,
            target_kind: runtime.target_kind,
            logical_model_id: runtime.logical_model_id,
            display_path: runtime.display_path,
            family: runtime.family,
            backend_id: runtime.backend_id,
            backend_class: runtime.backend_class,
            provider_id: runtime.provider_id,
            remote_model_id: runtime.remote_model_id,
            state: runtime.state,
            reservation_ram_bytes: runtime.reservation_ram_bytes,
            reservation_vram_bytes: runtime.reservation_vram_bytes,
            pinned: runtime.pinned,
            transition_state: runtime.transition_state,
            active_pid_count: runtime.active_pid_count,
            active_pids: runtime.active_pids,
            current: runtime.current,
        })
        .collect();
    let governor_status = deps.resource_governor.status(deps.runtime_registry);
    let governor_view = ResourceGovernorStatusView {
        ram_budget_bytes: governor_status.ram_budget_bytes,
        vram_budget_bytes: governor_status.vram_budget_bytes,
        min_ram_headroom_bytes: governor_status.min_ram_headroom_bytes,
        min_vram_headroom_bytes: governor_status.min_vram_headroom_bytes,
        ram_used_bytes: governor_status.ram_used_bytes,
        vram_used_bytes: governor_status.vram_used_bytes,
        ram_available_bytes: governor_status.ram_available_bytes,
        vram_available_bytes: governor_status.vram_available_bytes,
        pending_queue_depth: governor_status.pending_queue_depth,
        loader_busy: governor_status.loader_busy,
        loader_reason: governor_status.loader_reason,
    };
    let runtime_load_queue: Vec<RuntimeLoadQueueEntryView> = deps
        .resource_governor
        .queue_views()
        .into_iter()
        .map(|entry| RuntimeLoadQueueEntryView {
            queue_id: entry.queue_id,
            logical_model_id: entry.logical_model_id,
            display_path: entry.display_path,
            backend_class: entry.backend_class,
            state: entry.state,
            reservation_ram_bytes: entry.reservation_ram_bytes,
            reservation_vram_bytes: entry.reservation_vram_bytes,
            reason: entry.reason,
            requested_at_ms: entry.requested_at_ms,
            updated_at_ms: entry.updated_at_ms,
        })
        .collect();
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
        orchestrations: OrchestrationsStatus {
            active_orchestrations: build_orchestration_summaries(deps),
        },
        processes: processes_status,
    }
}

fn current_loaded_target_info(
    model_catalog: &ModelCatalog,
    loaded_path: &std::path::Path,
    loaded_remote_model: Option<&agentic_control_models::RemoteModelRuntimeView>,
) -> (String, String, Option<String>, Option<String>) {
    if let Some(model) = loaded_remote_model {
        return (
            model.model_id.clone(),
            "remote_provider".to_string(),
            Some(model.provider_id.clone()),
            Some(model.model_id.clone()),
        );
    }

    let loaded_path = loaded_path.to_string_lossy();
    if let Some(entry) = model_catalog
        .entries
        .iter()
        .find(|entry| entry.path.to_string_lossy() == loaded_path)
    {
        return (entry.id.clone(), "local_catalog".to_string(), None, None);
    }

    (
        loaded_path.to_string(),
        "local_path".to_string(),
        None,
        None,
    )
}

pub fn build_pid_status(deps: &StatusSnapshotDeps<'_>, pid: u64) -> Option<PidStatusResponse> {
    let engine = deps
        .runtime_registry
        .runtime_id_for_pid(pid)
        .and_then(|runtime_id| deps.runtime_registry.engine(runtime_id));
    build_pid_status_response_checked(deps, engine, pid)
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
        session_id: deps.session_registry.session_id_for_pid_or_fallback(pid),
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
        context_slot_id: None,
        resident_slot_policy: None,
        resident_slot_state: None,
        resident_slot_snapshot_path: None,
        backend_id: None,
        backend_class: None,
        backend_capabilities: None,
        session_accounting: None,
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
            let (backend_id, backend_class, backend_capabilities, _) =
                runtime_backend_status(&process.model);
            let session_id = deps.session_registry.session_id_for_pid_or_fallback(pid);
            let session_accounting = deps
                .storage
                .accounting_summary_for_session(&session_id)
                .ok()
                .flatten()
                .map(|summary| summary.into_view());
            return Some(PidStatusResponse {
                session_id,
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
                context_slot_id: process.context_slot_id,
                resident_slot_policy: process.resident_slot_policy_label(),
                resident_slot_state: process.resident_slot_state_label(),
                resident_slot_snapshot_path: process
                    .resident_slot_snapshot_path()
                    .map(|path| path.display().to_string()),
                backend_id,
                backend_class,
                backend_capabilities,
                session_accounting,
                context: Some(map_context_snapshot(process.context_status_snapshot())),
            });
        }
    }

    if let Some(checked_out) = deps.scheduler.checked_out_process(pid) {
        return Some(checked_out_pid_status_response(
            deps.session_registry.session_id_for_pid_or_fallback(pid),
            pid,
            sched.as_ref(),
            orchestration_binding,
            checked_out,
        ));
    }

    deps.scheduler.restored_process(pid).map(|metadata| {
        restored_pid_status_response(
            deps.session_registry.session_id_for_pid_or_fallback(pid),
            pid,
            sched.as_ref(),
            metadata,
        )
    })
}

fn checked_out_pid_status_response(
    session_id: String,
    pid: u64,
    sched: Option<&ProcessSchedulerSnapshot>,
    orchestration_binding: Option<(u64, String)>,
    metadata: &CheckedOutProcessMetadata,
) -> PidStatusResponse {
    PidStatusResponse {
        session_id,
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
        context_slot_id: metadata.context_slot_id,
        resident_slot_policy: metadata.resident_slot_policy.clone(),
        resident_slot_state: metadata.resident_slot_state.clone(),
        resident_slot_snapshot_path: metadata.resident_slot_snapshot_path.clone(),
        backend_id: metadata.backend_id.clone(),
        backend_class: metadata.backend_class.clone(),
        backend_capabilities: map_backend_capabilities(metadata.backend_capabilities),
        session_accounting: None,
        context: Some(map_context_snapshot(metadata.context.clone())),
    }
}

fn restored_pid_status_response(
    session_id: String,
    pid: u64,
    sched: Option<&ProcessSchedulerSnapshot>,
    metadata: &RestoredProcessMetadata,
) -> PidStatusResponse {
    PidStatusResponse {
        session_id,
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
        context_slot_id: metadata.context_slot_id,
        resident_slot_policy: metadata.resident_slot_policy.clone(),
        resident_slot_state: metadata.resident_slot_state.clone(),
        resident_slot_snapshot_path: metadata.resident_slot_snapshot_path.clone(),
        backend_id: metadata.backend_id.clone(),
        backend_class: metadata.backend_class.clone(),
        backend_capabilities: map_backend_capabilities(metadata.backend_capabilities),
        session_accounting: None,
        context: Some(map_context_snapshot(
            crate::process::ContextStatusSnapshot::from_parts(
                &metadata.context_policy,
                &metadata.context_state,
            ),
        )),
    }
}

fn runtime_backend_status(
    model: &RuntimeModel,
) -> (
    Option<String>,
    Option<String>,
    Option<BackendCapabilitiesView>,
    Option<BackendTelemetryView>,
) {
    (
        Some(model.backend_id().to_string()),
        Some(model.backend_class().as_str().to_string()),
        Some(model.backend_capabilities().into()),
        model.backend_telemetry(),
    )
}

fn map_backend_capabilities(
    capabilities: Option<BackendCapabilities>,
) -> Option<BackendCapabilitiesView> {
    capabilities.map(Into::into)
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
    use super::{
        build_global_status, checked_out_pid_status_response, collect_unique_pids,
        restored_pid_status_response, runtime_backend_status, StatusSnapshotDeps,
    };
    use crate::backend::{resolve_driver_for_model, TestOpenAIConfigOverrideGuard};
    use crate::backend::{
        ContextSlotPersistence, InferenceBackend, InferenceStepRequest, InferenceStepResult,
        RuntimeModel,
    };
    use crate::commands::MetricsState;
    use crate::config::OpenAIResponsesConfig;
    use crate::memory::NeuralMemory;
    use crate::model_catalog::{ModelCatalog, RemoteModelEntry, ResolvedModelTarget};
    use crate::orchestrator::Orchestrator;
    use crate::process::{ContextPolicy, ContextState, ContextStatusSnapshot, ContextStrategy};
    use crate::prompting::PromptFamily;
    use crate::resource_governor::ResourceGovernor;
    use crate::runtimes::{RuntimeRegistry, RuntimeReservation};
    use crate::scheduler::{CheckedOutProcessMetadata, ProcessScheduler, RestoredProcessMetadata};
    use crate::session::SessionRegistry;
    use crate::storage::StorageService;
    use anyhow::Result;
    use std::collections::HashSet;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn collect_unique_pids_preserves_first_seen_order() {
        let unique = collect_unique_pids([&[1, 2, 3], &[3, 4], &[2, 5], &[]]);
        assert_eq!(unique, vec![1, 2, 3, 4, 5]);
    }

    struct FakeResidentBackend;

    impl InferenceBackend for FakeResidentBackend {
        fn backend_id(&self) -> &'static str {
            "external-llamacpp"
        }

        fn family(&self) -> PromptFamily {
            PromptFamily::Qwen
        }

        fn generate_step(
            &mut self,
            _request: InferenceStepRequest<'_>,
        ) -> Result<InferenceStepResult> {
            panic!("generate_step should not be called in status snapshot tests");
        }

        fn duplicate_boxed(&self) -> Option<Box<dyn crate::backend::ModelBackend>> {
            None
        }
    }

    impl ContextSlotPersistence for FakeResidentBackend {}

    #[test]
    fn runtime_backend_status_reports_resident_backend_capabilities() {
        let model = RuntimeModel::from_boxed_backend(Box::new(FakeResidentBackend));

        let (backend_id, backend_class, backend_capabilities, backend_telemetry) =
            runtime_backend_status(&model);

        assert_eq!(backend_id.as_deref(), Some("external-llamacpp"));
        assert_eq!(backend_class.as_deref(), Some("resident_local"));
        assert_eq!(
            backend_capabilities
                .as_ref()
                .map(|capabilities| capabilities.persistent_slots),
            Some(true)
        );
        assert_eq!(
            backend_capabilities
                .as_ref()
                .map(|capabilities| capabilities.resident_kv),
            Some(true)
        );
        assert_eq!(backend_telemetry, None);
    }

    #[test]
    fn checked_out_status_preserves_backend_slot_metadata() {
        let policy = ContextPolicy::new(ContextStrategy::SlidingWindow, 256, 256, 128, 4);
        let context = ContextStatusSnapshot::from_parts(&policy, &ContextState::default());
        let response = checked_out_pid_status_response(
            "sess-test-000042".to_string(),
            42,
            None,
            None,
            &CheckedOutProcessMetadata {
                owner_id: 7,
                state: "InFlight".to_string(),
                tokens: 128,
                index_pos: 64,
                max_tokens: 512,
                context_slot_id: Some(9),
                resident_slot_policy: Some("park_and_resume".to_string()),
                resident_slot_state: Some("allocated".to_string()),
                resident_slot_snapshot_path: Some("workspace/swap/pid_42_slot_9.swap".to_string()),
                backend_id: Some("external-llamacpp".to_string()),
                backend_class: Some("resident_local".to_string()),
                backend_capabilities: Some(crate::backend::BackendCapabilities {
                    resident_kv: true,
                    persistent_slots: true,
                    save_restore_slots: true,
                    prompt_cache_reuse: true,
                    streaming_generation: false,
                    structured_output: false,
                    cancel_generation: false,
                    memory_telemetry: false,
                    tool_pause_resume: true,
                    context_compaction_reset: true,
                    parallel_sessions: true,
                }),
                context,
            },
        );

        assert_eq!(response.context_slot_id, Some(9));
        assert_eq!(
            response.resident_slot_policy.as_deref(),
            Some("park_and_resume")
        );
        assert_eq!(response.resident_slot_state.as_deref(), Some("allocated"));
        assert_eq!(
            response.resident_slot_snapshot_path.as_deref(),
            Some("workspace/swap/pid_42_slot_9.swap")
        );
        assert_eq!(response.backend_id.as_deref(), Some("external-llamacpp"));
        assert_eq!(response.backend_class.as_deref(), Some("resident_local"));
        assert_eq!(
            response
                .backend_capabilities
                .as_ref()
                .map(|capabilities| capabilities.save_restore_slots),
            Some(true)
        );
    }

    #[test]
    fn restored_status_can_surface_absent_backend_slot_metadata() {
        let response = restored_pid_status_response(
            "sess-test-000077".to_string(),
            77,
            None,
            &RestoredProcessMetadata {
                owner_id: 3,
                state: "Restored".to_string(),
                token_count: 32,
                max_tokens: 256,
                context_slot_id: None,
                resident_slot_policy: None,
                resident_slot_state: None,
                resident_slot_snapshot_path: None,
                backend_id: None,
                backend_class: None,
                backend_capabilities: None,
                context_policy: ContextPolicy::new(ContextStrategy::Summarize, 512, 384, 192, 4),
                context_state: ContextState::default(),
            },
        );

        assert_eq!(response.context_slot_id, None);
        assert_eq!(response.resident_slot_state, None);
        assert_eq!(response.resident_slot_snapshot_path, None);
        assert_eq!(response.backend_id, None);
        assert_eq!(response.backend_class, None);
        assert_eq!(response.backend_capabilities, None);
    }

    fn test_openai_config() -> OpenAIResponsesConfig {
        OpenAIResponsesConfig {
            endpoint: "http://127.0.0.1:19090/v1".to_string(),
            api_key: "test-key".to_string(),
            default_model: "gpt-4.1-mini".to_string(),
            timeout_ms: 5_000,
            max_request_bytes: 256 * 1024,
            max_response_bytes: 256 * 1024,
            stream: true,
            tokenizer_path: None,
            input_price_usd_per_mtok: 1.0,
            output_price_usd_per_mtok: 2.0,
            http_referer: String::new(),
            app_title: String::new(),
        }
    }

    #[test]
    fn global_status_surfaces_cloud_backend_metadata_for_lobby() {
        let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
        let driver_resolution =
            resolve_driver_for_model(PromptFamily::Unknown, None, Some("openai-responses"))
                .expect("resolve openai backend");
        let target = ResolvedModelTarget::remote(
            "openai-responses",
            "OpenAI",
            "openai-responses",
            "gpt-4.1-mini",
            RemoteModelEntry {
                id: "gpt-4.1-mini".to_string(),
                label: "GPT-4.1 mini".to_string(),
                context_window_tokens: None,
                max_output_tokens: None,
                supports_structured_output: true,
                input_price_usd_per_mtok: None,
                output_price_usd_per_mtok: None,
            },
            test_openai_config().into(),
            None,
            driver_resolution,
        );
        let memory = NeuralMemory::new().expect("memory init");
        let model_catalog =
            ModelCatalog::discover(crate::config::kernel_config().paths.models_dir.clone())
                .expect("discover model catalog");
        let scheduler = ProcessScheduler::new();
        let orchestrator = Orchestrator::new();
        let in_flight = HashSet::new();
        let metrics = MetricsState::new();
        let session_registry = fresh_session_registry();
        let (mut runtime_storage, mut runtime_registry, resource_governor) =
            fresh_runtime_registry();
        runtime_registry
            .activate_target(&mut runtime_storage, &target, RuntimeReservation::default())
            .expect("activate runtime");

        let status = build_global_status(&StatusSnapshotDeps {
            memory: &memory,
            runtime_registry: &runtime_registry,
            resource_governor: &resource_governor,
            model_catalog: &model_catalog,
            scheduler: &scheduler,
            orchestrator: &orchestrator,
            in_flight: &in_flight,
            metrics: &metrics,
            session_registry: &session_registry,
            storage: &runtime_storage,
        });

        assert!(status.model.loaded);
        assert_eq!(status.model.loaded_model_id, "gpt-4.1-mini");
        assert_eq!(
            status.model.loaded_target_kind.as_deref(),
            Some("remote_provider")
        );
        assert_eq!(
            status.model.loaded_provider_id.as_deref(),
            Some("openai-responses")
        );
        assert_eq!(
            status.model.loaded_remote_model_id.as_deref(),
            Some("gpt-4.1-mini")
        );
        assert_eq!(
            status.model.loaded_backend.as_deref(),
            Some("openai-responses")
        );
        assert_eq!(
            status.model.loaded_backend_class.as_deref(),
            Some("remote_stateless")
        );
        assert_eq!(
            status
                .model
                .loaded_backend_capabilities
                .as_ref()
                .map(|capabilities| capabilities.resident_kv),
            Some(false)
        );
        assert_eq!(
            status
                .model
                .loaded_remote_model
                .as_ref()
                .map(|model| model.model_id.as_str()),
            Some("gpt-4.1-mini")
        );
        assert!(status.model.loaded_backend_telemetry.is_some());
        assert_eq!(status.model.runtime_instances.len(), 1);
        assert!(status.model.resource_governor.is_some());
        assert_eq!(
            status.model.runtime_instances[0].backend_id,
            "openai-responses"
        );
    }

    fn fresh_session_registry() -> SessionRegistry {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("agenticos-status-snapshot-{unique}.db"));
        let mut storage = StorageService::open(&db_path).expect("open session registry storage");
        let boot = storage
            .record_kernel_boot("status-snapshot-test")
            .expect("record status snapshot boot");
        SessionRegistry::load(&mut storage, boot.boot_id).expect("load session registry")
    }

    fn fresh_runtime_registry() -> (StorageService, RuntimeRegistry, ResourceGovernor) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!("agenticos-runtime-snapshot-{unique}.db"));
        let mut storage = StorageService::open(&db_path).expect("open runtime registry storage");
        let registry = RuntimeRegistry::load(&mut storage).expect("load runtime registry");
        let governor = ResourceGovernor::load(
            &mut storage,
            crate::config::ResourceGovernorConfig::default(),
        )
        .expect("load resource governor");
        (storage, registry, governor)
    }
}
