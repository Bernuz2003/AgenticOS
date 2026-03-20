use std::collections::{HashMap, HashSet};

use agentic_control_models::{
    ArtifactListResponse, BackendCapabilitiesView, BackendTelemetryView,
    ContextStatusSnapshot as ControlContextStatusSnapshot, GenerationStatus, HumanInputRequestView,
    JobsStatus, MemoryStatus, ModelStatus, OrchArtifactRefView, OrchArtifactView,
    OrchStatusResponse, OrchSummaryResponse, OrchTaskAttemptView, OrchTaskEntry,
    OrchestrationListResponse, OrchestrationsStatus, PidStatusResponse, ProcessPermissionsView,
    ProcessesStatus, ResourceGovernorStatusView, RuntimeInstanceView, RuntimeLoadQueueEntryView,
    ScheduledJobListResponse, SchedulerStatus, StatusResponse,
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
use crate::services::job_scheduler::JobScheduler;
use crate::session::SessionRegistry;
use crate::storage::StorageService;

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
    let workflow_io = deps.storage.load_workflow_io(orch_id).ok()?;
    let (pending, running, completed, failed, skipped) = orch.counts();
    let total = orch.tasks.len();
    let elapsed = orch.created_at.elapsed().as_secs_f64();
    let finished = orch.is_finished();
    let attempts_by_task = workflow_io.attempts.into_iter().fold(
        HashMap::<String, Vec<crate::storage::StoredWorkflowTaskAttempt>>::new(),
        |mut acc, attempt| {
            acc.entry(attempt.task_id.clone())
                .or_default()
                .push(attempt);
            acc
        },
    );
    let artifacts_by_id = workflow_io
        .artifacts
        .iter()
        .cloned()
        .map(|artifact| (artifact.artifact_id.clone(), artifact))
        .collect::<HashMap<_, _>>();
    let artifacts_by_task = workflow_io.artifacts.into_iter().fold(
        HashMap::<String, Vec<crate::storage::StoredWorkflowArtifact>>::new(),
        |mut acc, artifact| {
            acc.entry(artifact.producer_task_id.clone())
                .or_default()
                .push(artifact);
            acc
        },
    );
    let inputs_by_attempt = workflow_io.inputs.into_iter().fold(
        HashMap::<(String, u32), Vec<crate::storage::StoredWorkflowArtifactInput>>::new(),
        |mut acc, input| {
            acc.entry((input.consumer_task_id.clone(), input.consumer_attempt))
                .or_default()
                .push(input);
            acc
        },
    );

    let tasks: Vec<OrchTaskEntry> = orch
        .topo_order
        .iter()
        .map(|task_id| {
            let status = &orch.status[task_id];
            let task_def = orch.tasks.get(task_id);
            let task_attempts = attempts_by_task.get(task_id).cloned().unwrap_or_default();
            let output_artifacts = artifacts_by_task.get(task_id).cloned().unwrap_or_default();
            let current_attempt = match status {
                crate::orchestrator::TaskStatus::Running { attempt, .. }
                | crate::orchestrator::TaskStatus::Completed { attempt }
                | crate::orchestrator::TaskStatus::Failed { attempt, .. } => Some(*attempt),
                crate::orchestrator::TaskStatus::Pending
                | crate::orchestrator::TaskStatus::Skipped => {
                    task_attempts.first().map(|attempt| attempt.attempt)
                }
            };
            let selected_attempt =
                current_attempt.or_else(|| task_attempts.first().map(|attempt| attempt.attempt));
            let input_artifacts = selected_attempt
                .and_then(|attempt| inputs_by_attempt.get(&(task_id.clone(), attempt)).cloned())
                .unwrap_or_default()
                .into_iter()
                .map(|input| {
                    let source = artifacts_by_id.get(&input.artifact_id);
                    OrchArtifactRefView {
                        artifact_id: input.artifact_id,
                        task: input.producer_task_id,
                        attempt: input.producer_attempt,
                        kind: source
                            .map(|artifact| artifact.kind.clone())
                            .unwrap_or_else(|| "task_output".to_string()),
                        label: source
                            .map(|artifact| artifact.label.clone())
                            .unwrap_or_else(|| "task artifact".to_string()),
                    }
                })
                .collect::<Vec<_>>();
            let artifact_views = output_artifacts
                .iter()
                .cloned()
                .map(|artifact| OrchArtifactView {
                    artifact_id: artifact.artifact_id,
                    task: artifact.producer_task_id,
                    attempt: artifact.producer_attempt,
                    kind: artifact.kind,
                    label: artifact.label,
                    mime_type: artifact.mime_type,
                    preview: artifact.preview,
                    content: artifact.content_text,
                    bytes: artifact.bytes,
                    created_at_ms: artifact.created_at_ms,
                })
                .collect::<Vec<_>>();
            let running_output = deps.orchestrator.running_output_for_task(orch_id, task_id);
            let (pid, error, context) = match status {
                crate::orchestrator::TaskStatus::Running { pid, .. } => (
                    Some(*pid),
                    None,
                    build_pid_status(deps, *pid).and_then(|response| response.context),
                ),
                crate::orchestrator::TaskStatus::Failed { error, .. } => {
                    (None, Some(error.clone()), None)
                }
                _ => (None, None, None),
            };
            let latest_output_preview = if let Some(running_output) = running_output {
                (!running_output.text.is_empty()).then_some(running_output.text.clone())
            } else {
                artifact_views
                    .first()
                    .map(|artifact| artifact.preview.clone())
            };
            let latest_output_text = if let Some(running_output) = running_output {
                (!running_output.text.is_empty()).then_some(running_output.text.clone())
            } else {
                artifact_views
                    .first()
                    .map(|artifact| artifact.content.clone())
            };
            let latest_output_truncated = running_output
                .map(|running_output| running_output.truncated)
                .unwrap_or_else(|| {
                    task_attempts
                        .first()
                        .map(|attempt| attempt.truncated)
                        .unwrap_or(false)
                });
            let attempts = task_attempts
                .into_iter()
                .map(|attempt| {
                    let running_preview = running_output
                        .filter(|running| running.attempt == attempt.attempt)
                        .map(|running| running.text.clone());
                    OrchTaskAttemptView {
                        attempt: attempt.attempt,
                        status: attempt.status,
                        session_id: attempt.session_id,
                        pid: attempt.pid,
                        error: attempt.error,
                        output_preview: running_preview
                            .clone()
                            .filter(|preview| !preview.is_empty())
                            .unwrap_or(attempt.output_preview),
                        output_chars: running_preview
                            .as_ref()
                            .map(|preview| preview.len())
                            .unwrap_or(attempt.output_chars),
                        truncated: running_output
                            .filter(|running| running.attempt == attempt.attempt)
                            .map(|running| running.truncated)
                            .unwrap_or(attempt.truncated),
                        started_at_ms: attempt.started_at_ms,
                        completed_at_ms: attempt.completed_at_ms,
                        primary_artifact_id: attempt.primary_artifact_id,
                    }
                })
                .collect::<Vec<_>>();
            OrchTaskEntry {
                task: task_id.clone(),
                role: task_def.and_then(|task| task.role.clone()),
                workload: task_def.and_then(|task| task.workload.clone()),
                backend_class: task_def
                    .map(|task| {
                        task.backend_class
                            .map(|backend_class| backend_class.as_str().to_string())
                    })
                    .flatten(),
                context_strategy: task_def.and_then(|task| task.context_strategy.clone()),
                deps: task_def.map(|task| task.deps.clone()).unwrap_or_default(),
                status: status.label().to_string(),
                current_attempt,
                pid,
                error,
                context,
                latest_output_preview,
                latest_output_text,
                latest_output_truncated,
                input_artifacts,
                output_artifacts: artifact_views,
                attempts,
            }
        })
        .collect();

    let truncations = tasks
        .iter()
        .flat_map(|task: &OrchTaskEntry| task.attempts.iter())
        .filter(|attempt| attempt.truncated)
        .count();
    let output_chars_stored = tasks
        .iter()
        .map(|task| {
            task.output_artifacts
                .iter()
                .map(|artifact| artifact.bytes)
                .sum::<usize>()
        })
        .sum::<usize>()
        + tasks
            .iter()
            .filter_map(|task| {
                if task.status == "running" {
                    task.latest_output_text.as_ref().map(|text| text.len())
                } else {
                    None
                }
            })
            .sum::<usize>();

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
        truncations,
        output_chars_stored,
        tasks,
    })
}

pub fn build_orchestration_list(deps: &StatusSnapshotDeps<'_>) -> OrchestrationListResponse {
    OrchestrationListResponse {
        orchestrations: build_orchestration_summaries(deps),
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

pub fn build_artifact_list(
    deps: &StatusSnapshotDeps<'_>,
    orch_id: u64,
    task_filter: Option<&str>,
) -> Option<ArtifactListResponse> {
    deps.orchestrator.get(orch_id)?;
    let workflow_io = deps.storage.load_workflow_io(orch_id).ok()?;
    let task_filter_owned = task_filter
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let artifacts = workflow_io
        .artifacts
        .into_iter()
        .filter(|artifact| {
            task_filter_owned
                .as_deref()
                .is_none_or(|task| artifact.producer_task_id == task)
        })
        .map(|artifact| OrchArtifactView {
            artifact_id: artifact.artifact_id,
            task: artifact.producer_task_id,
            attempt: artifact.producer_attempt,
            kind: artifact.kind,
            label: artifact.label,
            mime_type: artifact.mime_type,
            preview: artifact.preview,
            content: artifact.content_text,
            bytes: artifact.bytes,
            created_at_ms: artifact.created_at_ms,
        })
        .collect();

    Some(ArtifactListResponse {
        orchestration_id: orch_id,
        task: task_filter_owned,
        artifacts,
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
        tool_caller: String::new(),
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
        permissions: ProcessPermissionsView {
            trust_scope: "unknown".to_string(),
            actions_allowed: false,
            allowed_tools: Vec::new(),
            path_scopes: Vec::new(),
        },
        context: None,
        pending_human_request: None,
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
                tool_caller: process.tool_caller.as_str().to_string(),
                orchestration_id: orchestration_binding
                    .as_ref()
                    .map(|(orch_id, _, _)| *orch_id),
                orchestration_task_id: orchestration_binding
                    .as_ref()
                    .map(|(_, task_id, _)| task_id.clone()),
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
                permissions: map_permissions_view(&process.permission_policy),
                context: Some(map_context_snapshot(process.context_status_snapshot())),
                pending_human_request: process
                    .pending_human_request
                    .as_ref()
                    .map(map_human_input_request),
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
    orchestration_binding: Option<(u64, String, u32)>,
    metadata: &CheckedOutProcessMetadata,
) -> PidStatusResponse {
    PidStatusResponse {
        session_id,
        pid,
        owner_id: metadata.owner_id,
        tool_caller: metadata.tool_caller.as_str().to_string(),
        orchestration_id: orchestration_binding
            .as_ref()
            .map(|(orch_id, _, _)| *orch_id),
        orchestration_task_id: orchestration_binding
            .as_ref()
            .map(|(_, task_id, _)| task_id.clone()),
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
        permissions: map_permissions_view(&metadata.permission_policy),
        context: Some(map_context_snapshot(metadata.context.clone())),
        pending_human_request: metadata
            .pending_human_request
            .as_ref()
            .map(map_human_input_request),
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
        tool_caller: metadata.tool_caller.as_str().to_string(),
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
        permissions: map_permissions_view(&metadata.permission_policy),
        context: Some(map_context_snapshot(
            crate::process::ContextStatusSnapshot::from_parts(
                &metadata.context_policy,
                &metadata.context_state,
            ),
        )),
        pending_human_request: metadata
            .pending_human_request
            .as_ref()
            .map(map_human_input_request),
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

fn map_permissions_view(
    policy: &crate::tools::invocation::ProcessPermissionPolicy,
) -> ProcessPermissionsView {
    ProcessPermissionsView {
        trust_scope: policy.trust_scope.as_str().to_string(),
        actions_allowed: policy.actions_allowed,
        allowed_tools: policy.allowed_tools.clone(),
        path_scopes: policy.path_scopes.clone(),
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
        context_retrieval_requests: snapshot.context_retrieval_requests,
        context_retrieval_misses: snapshot.context_retrieval_misses,
        context_retrieval_candidates_scored: snapshot.context_retrieval_candidates_scored,
        context_retrieval_segments_selected: snapshot.context_retrieval_segments_selected,
        last_retrieval_candidates_scored: snapshot.last_retrieval_candidates_scored,
        last_retrieval_segments_selected: snapshot.last_retrieval_segments_selected,
        last_retrieval_latency_ms: snapshot.last_retrieval_latency_ms,
        last_retrieval_top_score: snapshot.last_retrieval_top_score,
        last_compaction_reason: snapshot.last_compaction_reason,
        last_summary_ts: snapshot.last_summary_ts,
        context_segments: snapshot.context_segments,
        episodic_segments: snapshot.episodic_segments,
        episodic_tokens: snapshot.episodic_tokens,
        retrieve_top_k: snapshot.retrieve_top_k,
        retrieve_candidate_limit: snapshot.retrieve_candidate_limit,
        retrieve_max_segment_chars: snapshot.retrieve_max_segment_chars,
        retrieve_min_score: snapshot.retrieve_min_score,
    }
}

fn map_human_input_request(request: &crate::process::HumanInputRequest) -> HumanInputRequestView {
    HumanInputRequestView {
        kind: request.kind.as_str().to_string(),
        question: request.question.clone(),
        details: request.details.clone(),
        choices: request.choices.clone(),
        allow_free_text: request.allow_free_text,
        placeholder: request.placeholder.clone(),
        requested_at_ms: request.requested_at_ms,
    }
}

fn build_orchestration_summaries(deps: &StatusSnapshotDeps<'_>) -> Vec<OrchSummaryResponse> {
    deps.orchestrator
        .all_ids()
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
#[path = "status_snapshot_tests.rs"]
mod tests;
