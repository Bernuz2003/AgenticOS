use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackendCapabilitiesView {
    pub resident_kv: bool,
    pub persistent_slots: bool,
    pub save_restore_slots: bool,
    pub prompt_cache_reuse: bool,
    pub streaming_generation: bool,
    pub structured_output: bool,
    pub cancel_generation: bool,
    pub memory_telemetry: bool,
    pub tool_pause_resume: bool,
    pub context_compaction_reset: bool,
    pub parallel_sessions: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackendTelemetryView {
    pub requests_total: u64,
    pub stream_requests_total: u64,
    pub input_tokens_total: u64,
    pub output_tokens_total: u64,
    pub estimated_cost_usd: f64,
    pub rate_limit_errors: u64,
    pub auth_errors: u64,
    pub transport_errors: u64,
    pub last_model: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteModelRuntimeView {
    pub provider_id: String,
    pub provider_label: String,
    pub backend_id: String,
    pub adapter_kind: String,
    pub model_id: String,
    pub model_label: String,
    pub context_window_tokens: Option<usize>,
    pub max_output_tokens: Option<usize>,
    pub supports_structured_output: bool,
    pub input_price_usd_per_mtok: Option<f64>,
    pub output_price_usd_per_mtok: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeInstanceView {
    pub runtime_id: String,
    pub target_kind: String,
    pub logical_model_id: String,
    pub display_path: String,
    pub family: String,
    pub backend_id: String,
    pub backend_class: String,
    pub provider_id: Option<String>,
    pub remote_model_id: Option<String>,
    pub state: String,
    pub reservation_ram_bytes: u64,
    pub reservation_vram_bytes: u64,
    pub pinned: bool,
    pub transition_state: Option<String>,
    pub active_pid_count: usize,
    pub active_pids: Vec<u64>,
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeLoadQueueEntryView {
    pub queue_id: i64,
    pub logical_model_id: String,
    pub display_path: String,
    pub backend_class: String,
    pub state: String,
    pub reservation_ram_bytes: u64,
    pub reservation_vram_bytes: u64,
    pub reason: String,
    pub requested_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManagedLocalRuntimeView {
    pub family: String,
    pub logical_model_id: String,
    pub display_path: String,
    pub state: String,
    pub endpoint: String,
    pub port: u16,
    pub context_window_tokens: Option<usize>,
    pub slot_save_dir: String,
    pub managed_by_kernel: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceGovernorStatusView {
    pub ram_budget_bytes: u64,
    pub vram_budget_bytes: u64,
    pub min_ram_headroom_bytes: u64,
    pub min_vram_headroom_bytes: u64,
    pub ram_used_bytes: u64,
    pub vram_used_bytes: u64,
    pub ram_available_bytes: u64,
    pub vram_available_bytes: u64,
    pub pending_queue_depth: usize,
    pub loader_busy: bool,
    pub loader_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlMessage {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecStartPayload {
    pub session_id: String,
    pub pid: u64,
    pub workload: String,
    pub priority: String,
    pub context_strategy: String,
    pub context_window_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeSessionResult {
    pub session_id: String,
    pub pid: u64,
    pub resumed_from_history: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrateResult {
    pub orchestration_id: u64,
    pub total_tasks: usize,
    pub spawned: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryTaskResult {
    pub orchestration_id: u64,
    pub task: String,
    pub reset_tasks: Vec<String>,
    pub spawned: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleJobResult {
    pub job_id: u64,
    pub next_run_at_ms: Option<i64>,
    pub trigger_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationControlResult {
    pub orchestration_id: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJobControlResult {
    pub job_id: u64,
    pub enabled: bool,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationStatusRequest {
    pub orchestration_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactListRequest {
    pub orchestration_id: u64,
    #[serde(default)]
    pub task: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreDumpRequest {
    #[serde(default)]
    pub pid: Option<u64>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub include_workspace: Option<bool>,
    #[serde(default)]
    pub include_backend_state: Option<bool>,
    #[serde(default)]
    pub freeze_target: Option<bool>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreDumpInfoRequest {
    pub dump_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreDumpListRequest {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreDumpReplayRequest {
    pub dump_id: String,
    #[serde(default)]
    pub branch_label: Option<String>,
    #[serde(default)]
    pub tool_mode: Option<String>,
    #[serde(default)]
    pub patch: Option<CoreDumpReplayPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CoreDumpReplayPatch {
    #[serde(default)]
    pub context_segments: Option<Vec<CoreDumpReplaySegmentPatch>>,
    #[serde(default)]
    pub episodic_segments: Option<Vec<CoreDumpReplaySegmentPatch>>,
    #[serde(default)]
    pub tool_output_overrides: Vec<CoreDumpReplayToolOutputOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreDumpReplaySegmentPatch {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CoreDumpReplayToolOutputOverride {
    pub tool_call_id: String,
    #[serde(default)]
    pub output: Option<Value>,
    #[serde(default)]
    pub output_text: Option<String>,
    #[serde(default)]
    pub warnings: Option<Vec<String>>,
    #[serde(default)]
    pub error_kind: Option<String>,
    #[serde(default)]
    pub error_text: Option<String>,
    #[serde(default)]
    pub effects: Option<Vec<Value>>,
    #[serde(default)]
    pub duration_ms: Option<u128>,
    #[serde(default)]
    pub kill: Option<bool>,
    #[serde(default)]
    pub success: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub uptime_secs: u64,
    pub total_commands: u64,
    pub total_errors: u64,
    pub total_exec_started: u64,
    pub total_signals: u64,
    #[serde(default)]
    pub global_accounting: Option<BackendTelemetryView>,
    #[serde(default)]
    pub mcp: Option<McpStatusView>,
    pub model: ModelStatus,
    pub generation: Option<GenerationStatus>,
    pub memory: MemoryStatus,
    pub scheduler: SchedulerStatus,
    pub jobs: JobsStatus,
    pub orchestrations: OrchestrationsStatus,
    pub processes: ProcessesStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStatus {
    pub loaded: bool,
    pub loaded_model_id: String,
    pub loaded_family: String,
    pub loaded_model_path: String,
    pub selected_model_id: String,
    pub loaded_target_kind: Option<String>,
    pub loaded_provider_id: Option<String>,
    pub loaded_remote_model_id: Option<String>,
    pub loaded_backend: Option<String>,
    pub loaded_backend_class: Option<String>,
    pub loaded_backend_capabilities: Option<BackendCapabilitiesView>,
    pub loaded_backend_telemetry: Option<BackendTelemetryView>,
    pub loaded_remote_model: Option<RemoteModelRuntimeView>,
    #[serde(default)]
    pub runtime_instances: Vec<RuntimeInstanceView>,
    #[serde(default)]
    pub managed_local_runtimes: Vec<ManagedLocalRuntimeView>,
    pub resource_governor: Option<ResourceGovernorStatusView>,
    #[serde(default)]
    pub runtime_load_queue: Vec<RuntimeLoadQueueEntryView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpStatusView {
    #[serde(default)]
    pub servers: Vec<McpServerStatusView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatusView {
    pub server_id: String,
    #[serde(default)]
    pub label: Option<String>,
    pub transport: String,
    pub trust_level: String,
    pub auth_mode: String,
    pub health: String,
    pub tool_prefix: String,
    pub enabled: bool,
    pub connected: bool,
    pub default_allowlisted: bool,
    pub approval_required: bool,
    pub roots_enabled: bool,
    #[serde(default)]
    pub exposed_tools: Vec<String>,
    #[serde(default)]
    pub discovered_tools: Vec<McpDiscoveredToolView>,
    #[serde(default)]
    pub prompts: Vec<McpPromptView>,
    #[serde(default)]
    pub resources: Vec<McpResourceView>,
    #[serde(default)]
    pub last_latency_ms: Option<u64>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpDiscoveredToolView {
    pub agentic_tool_name: String,
    pub target_name: String,
    #[serde(default)]
    pub title: Option<String>,
    pub description: String,
    pub dangerous: bool,
    pub default_allowlisted: bool,
    pub approval_required: bool,
    pub read_only_hint: bool,
    pub destructive_hint: bool,
    pub idempotent_hint: bool,
    pub open_world_hint: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptView {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceView {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    pub uri: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationStatus {
    pub temperature: f64,
    pub top_p: f64,
    pub seed: u64,
    pub max_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStatus {
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
    pub parked_pids: usize,
    pub oom_events: u64,
    pub swap_worker_crashes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerStatus {
    pub tracked: usize,
    pub priority_critical: usize,
    pub priority_high: usize,
    pub priority_normal: usize,
    pub priority_low: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobsStatus {
    #[serde(default)]
    pub scheduled_jobs: Vec<ScheduledJobView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJobView {
    pub job_id: u64,
    pub name: String,
    pub target_kind: String,
    pub trigger_kind: String,
    pub trigger_label: String,
    pub enabled: bool,
    pub state: String,
    #[serde(default)]
    pub next_run_at_ms: Option<i64>,
    #[serde(default)]
    pub current_trigger_at_ms: Option<i64>,
    pub current_attempt: u32,
    pub timeout_ms: u64,
    pub max_retries: u32,
    pub backoff_ms: u64,
    #[serde(default)]
    pub last_run_started_at_ms: Option<i64>,
    #[serde(default)]
    pub last_run_completed_at_ms: Option<i64>,
    #[serde(default)]
    pub last_run_status: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
    #[serde(default)]
    pub active_orchestration_id: Option<u64>,
    #[serde(default)]
    pub recent_runs: Vec<ScheduledJobRunView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJobRunView {
    pub run_id: u64,
    pub trigger_at_ms: i64,
    pub attempt: u32,
    pub status: String,
    #[serde(default)]
    pub started_at_ms: Option<i64>,
    #[serde(default)]
    pub completed_at_ms: Option<i64>,
    #[serde(default)]
    pub orchestration_id: Option<u64>,
    #[serde(default)]
    pub deadline_at_ms: Option<i64>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessesStatus {
    pub active_pids: Vec<u64>,
    pub parked_pids: Vec<u64>,
    pub in_flight_pids: Vec<u64>,
    pub active_processes: Vec<PidStatusResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationsStatus {
    pub active_orchestrations: Vec<OrchSummaryResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationListResponse {
    #[serde(default)]
    pub orchestrations: Vec<OrchSummaryResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchSummaryResponse {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanInputRequestView {
    pub request_id: String,
    pub kind: String,
    pub question: String,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub choices: Vec<String>,
    pub allow_free_text: bool,
    #[serde(default)]
    pub placeholder: Option<String>,
    pub requested_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PidStatusResponse {
    pub session_id: String,
    pub pid: u64,
    pub owner_id: usize,
    pub tool_caller: String,
    pub orchestration_id: Option<u64>,
    pub orchestration_task_id: Option<String>,
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
    pub context_slot_id: Option<u64>,
    pub resident_slot_policy: Option<String>,
    pub resident_slot_state: Option<String>,
    pub resident_slot_snapshot_path: Option<String>,
    pub backend_id: Option<String>,
    pub backend_class: Option<String>,
    pub backend_capabilities: Option<BackendCapabilitiesView>,
    #[serde(default)]
    pub session_accounting: Option<BackendTelemetryView>,
    pub permissions: ProcessPermissionsView,
    pub context: Option<ContextStatusSnapshot>,
    #[serde(default)]
    pub pending_human_request: Option<HumanInputRequestView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessPermissionsView {
    pub trust_scope: String,
    pub actions_allowed: bool,
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub path_grants: Vec<PathGrantView>,
    pub path_scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PathGrantAccessMode {
    ReadOnly,
    WriteApproved,
    AutonomousWrite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PathGrantView {
    pub root: String,
    pub access_mode: PathGrantAccessMode,
    #[serde(default)]
    pub capsule: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    pub workspace_relative: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextStatusSnapshot {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchArtifactRefView {
    pub artifact_id: String,
    pub task: String,
    pub attempt: u32,
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchArtifactView {
    pub artifact_id: String,
    pub task: String,
    pub attempt: u32,
    pub kind: String,
    pub label: String,
    pub mime_type: String,
    pub preview: String,
    pub content: String,
    pub bytes: usize,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcMessageView {
    pub message_id: String,
    #[serde(default)]
    pub orchestration_id: Option<u64>,
    #[serde(default)]
    pub sender_pid: Option<u64>,
    #[serde(default)]
    pub sender_task: Option<String>,
    #[serde(default)]
    pub sender_attempt: Option<u32>,
    #[serde(default)]
    pub receiver_pid: Option<u64>,
    #[serde(default)]
    pub receiver_task: Option<String>,
    #[serde(default)]
    pub receiver_attempt: Option<u32>,
    #[serde(default)]
    pub receiver_role: Option<String>,
    pub message_type: String,
    #[serde(default)]
    pub channel: Option<String>,
    pub payload_preview: String,
    pub payload_text: String,
    pub status: String,
    pub created_at_ms: i64,
    #[serde(default)]
    pub delivered_at_ms: Option<i64>,
    #[serde(default)]
    pub consumed_at_ms: Option<i64>,
    #[serde(default)]
    pub failed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchTaskAttemptView {
    pub attempt: u32,
    pub status: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub pid: Option<u64>,
    #[serde(default)]
    pub error: Option<String>,
    pub output_preview: String,
    pub output_chars: usize,
    pub truncated: bool,
    pub started_at_ms: i64,
    #[serde(default)]
    pub completed_at_ms: Option<i64>,
    #[serde(default)]
    pub primary_artifact_id: Option<String>,
    #[serde(default)]
    pub termination_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchStatusResponse {
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
    #[serde(default)]
    pub ipc_messages: Vec<IpcMessageView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJobListResponse {
    #[serde(default)]
    pub jobs: Vec<ScheduledJobView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactListResponse {
    pub orchestration_id: u64,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<OrchArtifactView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoreDumpSummaryView {
    pub dump_id: String,
    pub created_at_ms: i64,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub pid: Option<u64>,
    pub reason: String,
    pub fidelity: String,
    pub path: String,
    pub bytes: usize,
    pub sha256: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreDumpCaptureResult {
    pub dump: CoreDumpSummaryView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreDumpListResponse {
    #[serde(default)]
    pub dumps: Vec<CoreDumpSummaryView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreDumpInfoResponse {
    pub dump: CoreDumpSummaryView,
    pub manifest_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreDumpReplayResult {
    pub source_dump_id: String,
    pub session_id: String,
    pub pid: u64,
    pub runtime_id: String,
    pub replay_session_title: String,
    pub replay_fidelity: String,
    pub replay_mode: String,
    pub tool_mode: String,
    pub initial_state: String,
    pub patched_context_segments: usize,
    pub patched_episodic_segments: usize,
    pub stubbed_invocations: usize,
    pub overridden_invocations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchTaskEntry {
    pub task: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub workload: Option<String>,
    #[serde(default)]
    pub backend_class: Option<String>,
    #[serde(default)]
    pub context_strategy: Option<String>,
    #[serde(default)]
    pub deps: Vec<String>,
    pub status: String,
    #[serde(default)]
    pub current_attempt: Option<u32>,
    pub pid: Option<u64>,
    pub error: Option<String>,
    pub context: Option<ContextStatusSnapshot>,
    #[serde(default)]
    pub latest_output_preview: Option<String>,
    #[serde(default)]
    pub latest_output_text: Option<String>,
    #[serde(default)]
    pub latest_output_truncated: bool,
    #[serde(default)]
    pub input_artifacts: Vec<OrchArtifactRefView>,
    #[serde(default)]
    pub output_artifacts: Vec<OrchArtifactView>,
    #[serde(default)]
    pub attempts: Vec<OrchTaskAttemptView>,
    #[serde(default)]
    pub termination_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalogSnapshot {
    pub selected_model_id: Option<String>,
    pub total_models: usize,
    pub models: Vec<ModelCatalogEntry>,
    pub routing_recommendations: Vec<ModelRoutingRecommendation>,
    #[serde(default)]
    pub remote_providers: Vec<RemoteProviderCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteProviderCatalogEntry {
    pub id: String,
    pub backend_id: String,
    pub adapter_kind: String,
    pub label: String,
    pub note: Option<String>,
    pub credential_hint: Option<String>,
    pub default_model_id: String,
    pub models: Vec<RemoteModelCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteModelCatalogEntry {
    pub id: String,
    pub label: String,
    pub context_window_tokens: Option<usize>,
    pub max_output_tokens: Option<usize>,
    pub supports_structured_output: bool,
    pub input_price_usd_per_mtok: Option<f64>,
    pub output_price_usd_per_mtok: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalogEntry {
    pub id: String,
    pub family: String,
    pub architecture: Option<String>,
    pub max_context_tokens: Option<usize>,
    pub path: String,
    pub tokenizer_path: Option<String>,
    pub tokenizer_present: bool,
    pub metadata_source: Option<String>,
    pub backend_preference: Option<String>,
    pub resolved_backend: Option<String>,
    pub resolved_backend_class: Option<String>,
    pub resolved_backend_capabilities: Option<BackendCapabilitiesView>,
    pub driver_resolution_source: String,
    pub driver_resolution_rationale: String,
    pub driver_available: Option<bool>,
    pub driver_load_supported: Option<bool>,
    pub capabilities: Option<BTreeMap<String, f64>>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRoutingRecommendation {
    pub workload: String,
    pub model_id: Option<String>,
    pub family: Option<String>,
    pub backend_preference: Option<String>,
    pub resolved_backend: Option<String>,
    pub resolved_backend_class: Option<String>,
    pub resolved_backend_capabilities: Option<BackendCapabilitiesView>,
    pub driver_resolution_source: String,
    pub driver_resolution_rationale: String,
    pub driver_available: Option<bool>,
    pub driver_load_supported: Option<bool>,
    pub metadata_source: Option<String>,
    pub source: String,
    pub rationale: String,
    pub capability_key: Option<String>,
    pub capability_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfoResponse {
    pub id: String,
    pub family: String,
    pub architecture: Option<String>,
    pub max_context_tokens: Option<usize>,
    pub path: String,
    pub tokenizer_path: Option<String>,
    pub tokenizer_present: bool,
    pub metadata_source: Option<String>,
    pub backend_preference: Option<String>,
    pub resolved_backend: Option<String>,
    pub resolved_backend_class: Option<String>,
    pub resolved_backend_capabilities: Option<BackendCapabilitiesView>,
    pub driver_resolution_source: String,
    pub driver_resolution_rationale: String,
    pub driver_available: Option<bool>,
    pub driver_load_supported: Option<bool>,
    pub chat_template: Option<String>,
    pub assistant_preamble: Option<String>,
    pub special_tokens: Option<BTreeMap<String, String>>,
    pub stop_markers: Option<Vec<String>>,
    pub capabilities: Option<BTreeMap<String, f64>>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectModelResult {
    pub selected_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadModelResult {
    pub family: String,
    pub loaded_model_id: String,
    pub loaded_target_kind: String,
    pub loaded_provider_id: Option<String>,
    pub loaded_remote_model_id: Option<String>,
    pub backend: String,
    pub backend_class: String,
    pub backend_capabilities: BackendCapabilitiesView,
    pub driver_source: String,
    pub driver_rationale: String,
    pub path: String,
    pub architecture: Option<String>,
    pub load_mode: String,
    pub remote_model: Option<RemoteModelRuntimeView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendInputResult {
    pub pid: u64,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnControlResult {
    pub pid: u64,
    pub state: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeResult {
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelEventEnvelope {
    pub seq: u64,
    pub event: KernelEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticEvent {
    pub category: String,
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub recorded_at_ms: i64,
    pub session_id: Option<String>,
    pub pid: Option<u64>,
    pub runtime_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InvocationKind {
    Tool,
    Action,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantSegmentKind {
    Message,
    Thinking,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InvocationStatus {
    Dispatched,
    Completed,
    Failed,
    Killed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvocationEvent {
    pub invocation_id: String,
    pub kind: InvocationKind,
    pub command: String,
    pub status: InvocationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KernelEvent {
    CoreDumpCreated {
        pid: u64,
        #[serde(default)]
        session_id: Option<String>,
        dump_id: String,
        reason: String,
        fidelity: String,
    },
    LobbyChanged {
        reason: String,
    },
    WorkspaceChanged {
        pid: u64,
        reason: String,
    },
    SessionStarted {
        session_id: String,
        pid: u64,
        workload: String,
        prompt: String,
    },
    TimelineSegment {
        pid: u64,
        segment_kind: AssistantSegmentKind,
        text: String,
    },
    InvocationUpdated {
        pid: u64,
        invocation: InvocationEvent,
    },
    SessionFinished {
        pid: u64,
        tokens_generated: Option<u64>,
        elapsed_secs: Option<f64>,
        reason: String,
    },
    SessionErrored {
        pid: u64,
        message: String,
    },
    DiagnosticRecorded {
        event: DiagnosticEvent,
    },
    ModelChanged {
        selected_model_id: String,
        loaded_model_id: String,
    },
    KernelShutdownRequested,
}
