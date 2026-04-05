use agentic_control_models::{
    ContextStatusSnapshot as ControlContextStatusSnapshot, HumanInputRequestView,
    PidStatusResponse, ProcessPermissionsView,
};

use crate::engine::LLMEngine;
use crate::scheduler::{
    CheckedOutProcessMetadata, ProcessSchedulerSnapshot, RestoredProcessMetadata,
};

use super::runtime::{map_backend_capabilities, runtime_backend_status};
use super::view::StatusSnapshotDeps;

pub fn build_pid_status(deps: &StatusSnapshotDeps<'_>, pid: u64) -> Option<PidStatusResponse> {
    let engine = deps
        .runtime_registry
        .runtime_id_for_pid(pid)
        .and_then(|runtime_id| deps.runtime_registry.engine(runtime_id));
    build_pid_status_response_checked(deps, engine, pid)
}

pub(super) fn build_pid_status_or_placeholder(
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

pub(crate) fn checked_out_pid_status_response(
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
        tokens: metadata.token_count,
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

pub(crate) fn restored_pid_status_response(
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
        request_id: request.request_id.clone(),
        kind: request.kind.as_str().to_string(),
        question: request.question.clone(),
        details: request.details.clone(),
        choices: request.choices.clone(),
        allow_free_text: request.allow_free_text,
        placeholder: request.placeholder.clone(),
        requested_at_ms: request.requested_at_ms,
    }
}
