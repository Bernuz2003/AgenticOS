use serde::{Deserialize, Serialize};

use agentic_control_models::{
    BackendCapabilitiesView, BackendTelemetryView, ProcessPermissionsView,
};

use super::workflows::WorkspaceOrchestrationSnapshot;
use crate::models::runtime::AuditEvent;

#[derive(Debug, Serialize, Clone)]
pub struct AgentSessionSummary {
    pub session_id: String,
    pub pid: u64,
    pub active_pid: Option<u64>,
    pub last_pid: Option<u64>,
    pub title: String,
    pub prompt_preview: String,
    pub status: String,
    pub runtime_state: Option<String>,
    pub uptime_label: String,
    pub tokens_label: String,
    pub context_strategy: String,
    pub runtime_id: Option<String>,
    pub runtime_label: Option<String>,
    pub backend_class: Option<String>,
    pub orchestration_id: Option<u64>,
    pub orchestration_task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPathGrantAccessMode {
    ReadOnly,
    WriteApproved,
    AutonomousWrite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPathGrantInput {
    pub root: String,
    pub access_mode: SessionPathGrantAccessMode,
    #[serde(default)]
    pub capsule: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceSnapshot {
    pub session_id: String,
    pub pid: u64,
    pub active_pid: Option<u64>,
    pub last_pid: Option<u64>,
    pub title: String,
    pub runtime_id: Option<String>,
    pub runtime_label: Option<String>,
    pub state: String,
    pub workload: String,
    pub owner_id: Option<usize>,
    pub tool_caller: Option<String>,
    pub index_pos: Option<usize>,
    pub priority: Option<String>,
    pub quota_tokens: Option<u64>,
    pub quota_syscalls: Option<u64>,
    pub context_slot_id: Option<u64>,
    pub resident_slot_policy: Option<String>,
    pub resident_slot_state: Option<String>,
    pub resident_slot_snapshot_path: Option<String>,
    pub backend_id: Option<String>,
    pub backend_class: Option<String>,
    pub backend_capabilities: Option<BackendCapabilitiesView>,
    pub accounting: Option<BackendTelemetryView>,
    pub permissions: Option<ProcessPermissionsView>,
    pub tokens_generated: usize,
    pub syscalls_used: usize,
    pub elapsed_secs: f64,
    pub tokens: usize,
    pub max_tokens: usize,
    pub orchestration: Option<WorkspaceOrchestrationSnapshot>,
    pub context: Option<WorkspaceContextSnapshot>,
    pub pending_human_request: Option<WorkspaceHumanInputRequest>,
    pub audit_events: Vec<AuditEvent>,
    pub lineage: Option<WorkspaceLineageSnapshot>,
    pub replay: Option<WorkspaceReplayDebugSnapshot>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceLineageBranchKind {
    Base,
    Replay,
    Fork,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceLineageBranch {
    pub session_id: String,
    pub kind: WorkspaceLineageBranchKind,
    pub title: String,
    pub created_at_ms: i64,
    pub active_pid: Option<u64>,
    pub last_pid: Option<u64>,
    pub source_dump_id: Option<String>,
    pub selected: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceLineageSnapshot {
    pub anchor_session_id: String,
    pub selected_session_id: String,
    pub selected_kind: WorkspaceLineageBranchKind,
    pub branches: Vec<WorkspaceLineageBranch>,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceContextSnapshot {
    pub context_strategy: String,
    pub context_tokens_used: usize,
    pub context_window_size: usize,
    pub context_compressions: u64,
    pub context_retrieval_hits: u64,
    pub context_retrieval_requests: u64,
    pub context_retrieval_misses: u64,
    pub context_retrieval_candidates_scored: u64,
    pub context_retrieval_segments_selected: u64,
    pub last_retrieval_candidates_scored: usize,
    pub last_retrieval_segments_selected: usize,
    pub last_retrieval_latency_ms: u64,
    pub last_retrieval_top_score: Option<f64>,
    pub last_compaction_reason: Option<String>,
    pub last_summary_ts: Option<String>,
    pub context_segments: usize,
    pub episodic_segments: usize,
    pub episodic_tokens: usize,
    pub retrieve_top_k: usize,
    pub retrieve_candidate_limit: usize,
    pub retrieve_max_segment_chars: usize,
    pub retrieve_min_score: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceHumanInputRequest {
    pub request_id: String,
    pub kind: String,
    pub question: String,
    pub details: Option<String>,
    pub choices: Vec<String>,
    pub allow_free_text: bool,
    pub placeholder: Option<String>,
    pub requested_at_ms: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceReplayDebugSnapshot {
    pub source_dump_id: String,
    pub source_session_id: Option<String>,
    pub source_pid: Option<u64>,
    pub source_fidelity: String,
    pub replay_mode: String,
    pub tool_mode: String,
    pub initial_state: String,
    pub patched_context_segments: usize,
    pub patched_episodic_segments: usize,
    pub stubbed_invocations: usize,
    pub overridden_invocations: usize,
    pub baseline: WorkspaceReplayBaselineSnapshot,
    pub diff: WorkspaceReplayDiffSnapshot,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceReplayBaselineSnapshot {
    pub source_context_segments: usize,
    pub source_episodic_segments: usize,
    pub source_replay_messages: usize,
    pub source_tool_invocations: usize,
    pub source_context_chars: usize,
    pub source_episodic_chars: usize,
    pub source_context_kinds: Vec<String>,
    pub source_episodic_kinds: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceReplayDiffSnapshot {
    pub current_context_segments: Option<usize>,
    pub current_episodic_segments: Option<usize>,
    pub current_replay_messages: usize,
    pub current_tool_invocations: usize,
    pub context_changed: Option<bool>,
    pub context_segments_delta: Option<i64>,
    pub episodic_segments_delta: Option<i64>,
    pub replay_messages_delta: i64,
    pub tool_invocations_delta: i64,
    pub branch_only_messages: usize,
    pub branch_only_tool_calls: usize,
    pub changed_tool_outputs: usize,
    pub completed_tool_calls: usize,
    pub latest_branch_message: Option<String>,
    pub invocation_diffs: Vec<WorkspaceReplayInvocationDiff>,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceReplayInvocationDiff {
    pub source_tool_call_id: Option<String>,
    pub replay_tool_call_id: Option<String>,
    pub tool_name: String,
    pub command_text: String,
    pub source_status: Option<String>,
    pub replay_status: Option<String>,
    pub source_output_text: Option<String>,
    pub replay_output_text: Option<String>,
    pub branch_only: bool,
    pub changed: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct StartSessionResult {
    pub session_id: String,
    pub pid: u64,
}
