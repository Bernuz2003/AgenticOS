use agentic_control_models::{OrchStatusResponse, OrchSummaryResponse, PidStatusResponse};

use crate::models::kernel::{
    AgentSessionSummary, LobbyOrchestrationSummary, WorkspaceContextSnapshot,
    WorkspaceHumanInputRequest, WorkspaceOrchestrationSnapshot, WorkspaceOrchestrationTask,
    WorkspaceSnapshot,
};
use crate::utils::formatting::compact_token_count;
use crate::utils::time::duration_label;

pub(super) fn map_process_to_session(process: PidStatusResponse) -> AgentSessionSummary {
    let status = match process.state.as_str() {
        "Running" | "WaitingForSyscall" | "InFlight" | "AwaitingRemoteResponse" => "running",
        "Parked" => "swapped",
        _ => "idle",
    }
    .to_string();

    let context_strategy = process
        .context
        .as_ref()
        .map(|context| context.context_strategy.clone())
        .unwrap_or_else(|| "sliding_window".to_string());

    let prompt_preview = if let Some(context) = process.context.as_ref() {
        format!(
            "workload={} | backend={} | slot={} [{} / {}] | context={}/{} tokens | strategy={}",
            process.workload,
            process.backend_class.as_deref().unwrap_or("unknown"),
            process
                .context_slot_id
                .map(|slot_id| slot_id.to_string())
                .unwrap_or_else(|| "none".to_string()),
            process
                .resident_slot_policy
                .as_deref()
                .unwrap_or("unmanaged"),
            process.resident_slot_state.as_deref().unwrap_or("unbound"),
            context.context_tokens_used,
            context.context_window_size,
            context.context_strategy
        )
    } else {
        format!(
            "workload={} | backend={} | slot={} [{} / {}] | no context snapshot available",
            process.workload,
            process.backend_class.as_deref().unwrap_or("unknown"),
            process
                .context_slot_id
                .map(|slot_id| slot_id.to_string())
                .unwrap_or_else(|| "none".to_string()),
            process
                .resident_slot_policy
                .as_deref()
                .unwrap_or("unmanaged"),
            process.resident_slot_state.as_deref().unwrap_or("unbound"),
        )
    };

    AgentSessionSummary {
        session_id: process.session_id.clone(),
        pid: process.pid,
        active_pid: Some(process.pid),
        last_pid: Some(process.pid),
        title: format!("{} / PID {}", process.workload, process.pid),
        prompt_preview,
        status,
        runtime_state: Some(process.state.clone()),
        uptime_label: duration_label(process.elapsed_secs),
        tokens_label: compact_token_count(process.tokens_generated),
        context_strategy,
        runtime_id: None,
        runtime_label: process
            .backend_id
            .as_ref()
            .map(|backend_id| format!("{backend_id} · {}", process.workload)),
        backend_class: process.backend_class.clone(),
        orchestration_id: process.orchestration_id,
        orchestration_task_id: process.orchestration_task_id,
    }
}

pub(super) fn map_orchestration_to_summary(
    orchestration: OrchSummaryResponse,
) -> LobbyOrchestrationSummary {
    LobbyOrchestrationSummary {
        orchestration_id: orchestration.orchestration_id,
        total: orchestration.total,
        completed: orchestration.completed,
        running: orchestration.running,
        pending: orchestration.pending,
        failed: orchestration.failed,
        skipped: orchestration.skipped,
        finished: orchestration.finished,
        elapsed_label: duration_label(orchestration.elapsed_secs),
        policy: orchestration.policy,
    }
}

pub(super) fn map_orchestration_status_to_workspace_snapshot(
    orchestration: OrchStatusResponse,
    task_id: Option<String>,
) -> WorkspaceOrchestrationSnapshot {
    WorkspaceOrchestrationSnapshot {
        orchestration_id: orchestration.orchestration_id,
        task_id: task_id.unwrap_or_default(),
        total: orchestration.total,
        completed: orchestration.completed,
        running: orchestration.running,
        pending: orchestration.pending,
        failed: orchestration.failed,
        skipped: orchestration.skipped,
        finished: orchestration.finished,
        elapsed_secs: orchestration.elapsed_secs,
        policy: orchestration.policy,
        tasks: orchestration
            .tasks
            .into_iter()
            .map(|task| WorkspaceOrchestrationTask {
                task: task.task,
                status: task.status,
                pid: task.pid,
            })
            .collect(),
    }
}

pub(super) fn map_pid_status_to_workspace_snapshot(
    process: PidStatusResponse,
    orchestration: Option<WorkspaceOrchestrationSnapshot>,
    _orchestration_fetch_error: Option<String>,
) -> WorkspaceSnapshot {
    let context = process
        .context
        .as_ref()
        .map(|context| WorkspaceContextSnapshot {
            context_strategy: context.context_strategy.clone(),
            context_tokens_used: context.context_tokens_used,
            context_window_size: context.context_window_size,
            context_compressions: context.context_compressions,
            context_retrieval_hits: context.context_retrieval_hits,
            context_retrieval_requests: context.context_retrieval_requests,
            context_retrieval_misses: context.context_retrieval_misses,
            context_retrieval_candidates_scored: context.context_retrieval_candidates_scored,
            context_retrieval_segments_selected: context.context_retrieval_segments_selected,
            last_retrieval_candidates_scored: context.last_retrieval_candidates_scored,
            last_retrieval_segments_selected: context.last_retrieval_segments_selected,
            last_retrieval_latency_ms: context.last_retrieval_latency_ms,
            last_retrieval_top_score: context.last_retrieval_top_score,
            last_compaction_reason: context.last_compaction_reason.clone(),
            last_summary_ts: context.last_summary_ts.clone(),
            context_segments: context.context_segments,
            episodic_segments: context.episodic_segments,
            episodic_tokens: context.episodic_tokens,
            retrieve_top_k: context.retrieve_top_k,
            retrieve_candidate_limit: context.retrieve_candidate_limit,
            retrieve_max_segment_chars: context.retrieve_max_segment_chars,
            retrieve_min_score: context.retrieve_min_score,
        });

    WorkspaceSnapshot {
        session_id: process.session_id,
        pid: process.pid,
        active_pid: Some(process.pid),
        last_pid: Some(process.pid),
        title: format!("{} / PID {}", process.workload, process.pid),
        runtime_id: None,
        runtime_label: process
            .backend_id
            .as_ref()
            .map(|backend_id| format!("{backend_id} · {}", process.workload)),
        state: process.state,
        workload: process.workload,
        owner_id: (process.owner_id != 0).then_some(process.owner_id),
        tool_caller: Some(process.tool_caller),
        index_pos: Some(process.index_pos),
        priority: (!process.priority.trim().is_empty()).then_some(process.priority),
        quota_tokens: Some(process.quota_tokens),
        quota_syscalls: Some(process.quota_syscalls),
        context_slot_id: process.context_slot_id,
        resident_slot_policy: process.resident_slot_policy,
        resident_slot_state: process.resident_slot_state,
        resident_slot_snapshot_path: process.resident_slot_snapshot_path,
        backend_id: process.backend_id,
        backend_class: process.backend_class,
        backend_capabilities: process.backend_capabilities,
        accounting: process.session_accounting,
        permissions: Some(process.permissions),
        tokens_generated: process.tokens_generated,
        syscalls_used: process.syscalls_used,
        elapsed_secs: process.elapsed_secs,
        tokens: process.tokens,
        max_tokens: process.max_tokens,
        orchestration,
        context,
        pending_human_request: process.pending_human_request.map(|request| {
            WorkspaceHumanInputRequest {
                request_id: request.request_id,
                kind: request.kind,
                question: request.question,
                details: request.details,
                choices: request.choices,
                allow_free_text: request.allow_free_text,
                placeholder: request.placeholder,
                requested_at_ms: request.requested_at_ms,
            }
        }),
        audit_events: Vec::new(),
        replay: None,
    }
}

pub(super) fn merge_live_session_summary(
    persisted: Option<AgentSessionSummary>,
    live: AgentSessionSummary,
) -> AgentSessionSummary {
    let Some(persisted) = persisted else {
        return live;
    };

    AgentSessionSummary {
        session_id: live.session_id,
        pid: live.pid,
        active_pid: live.active_pid.or(persisted.active_pid),
        last_pid: live.last_pid.or(persisted.last_pid),
        title: if persisted.title.trim().is_empty() {
            live.title
        } else {
            persisted.title
        },
        prompt_preview: if persisted.prompt_preview.trim().is_empty() {
            live.prompt_preview
        } else {
            persisted.prompt_preview
        },
        status: live.status,
        runtime_state: live.runtime_state.or(persisted.runtime_state),
        uptime_label: live.uptime_label,
        tokens_label: live.tokens_label,
        context_strategy: if persisted.context_strategy.trim().is_empty() {
            live.context_strategy
        } else {
            persisted.context_strategy
        },
        runtime_id: persisted.runtime_id.or(live.runtime_id),
        runtime_label: persisted.runtime_label.or(live.runtime_label),
        backend_class: live.backend_class.or(persisted.backend_class),
        orchestration_id: live.orchestration_id,
        orchestration_task_id: live.orchestration_task_id,
    }
}
