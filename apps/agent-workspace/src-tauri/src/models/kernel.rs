use serde::Serialize;

use agentic_control_models::{
    BackendCapabilitiesView, BackendTelemetryView, MemoryStatus, RemoteModelRuntimeView,
    ResourceGovernorStatusView, RuntimeInstanceView, RuntimeLoadQueueEntryView,
};

#[derive(Debug, Serialize, Clone)]
pub struct KernelBootstrapState {
    pub kernel_addr: String,
    pub workspace_root: String,
    pub protocol_version: String,
    pub connection_mode: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct LobbySnapshot {
    pub connected: bool,
    pub selected_model_id: String,
    pub loaded_model_id: String,
    pub loaded_target_kind: Option<String>,
    pub loaded_provider_id: Option<String>,
    pub loaded_remote_model_id: Option<String>,
    pub loaded_backend_id: Option<String>,
    pub loaded_backend_class: Option<String>,
    pub loaded_backend_capabilities: Option<BackendCapabilitiesView>,
    pub global_accounting: Option<BackendTelemetryView>,
    pub loaded_backend_telemetry: Option<BackendTelemetryView>,
    pub loaded_remote_model: Option<RemoteModelRuntimeView>,
    pub memory: Option<MemoryStatus>,
    pub runtime_instances: Vec<RuntimeInstanceView>,
    pub resource_governor: Option<ResourceGovernorStatusView>,
    pub runtime_load_queue: Vec<RuntimeLoadQueueEntryView>,
    pub global_audit_events: Vec<AuditEvent>,
    pub orchestrations: Vec<LobbyOrchestrationSummary>,
    pub sessions: Vec<AgentSessionSummary>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AgentSessionSummary {
    pub session_id: String,
    pub pid: u64,
    pub active_pid: Option<u64>,
    pub last_pid: Option<u64>,
    pub title: String,
    pub prompt_preview: String,
    pub status: String,
    pub uptime_label: String,
    pub tokens_label: String,
    pub context_strategy: String,
    pub runtime_id: Option<String>,
    pub runtime_label: Option<String>,
    pub backend_class: Option<String>,
    pub orchestration_id: Option<u64>,
    pub orchestration_task_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct LobbyOrchestrationSummary {
    pub orchestration_id: u64,
    pub total: usize,
    pub completed: usize,
    pub running: usize,
    pub pending: usize,
    pub failed: usize,
    pub skipped: usize,
    pub finished: bool,
    pub elapsed_label: String,
    pub policy: String,
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
    pub context_slot_id: Option<u64>,
    pub resident_slot_policy: Option<String>,
    pub resident_slot_state: Option<String>,
    pub resident_slot_snapshot_path: Option<String>,
    pub backend_id: Option<String>,
    pub backend_class: Option<String>,
    pub backend_capabilities: Option<BackendCapabilitiesView>,
    pub accounting: Option<BackendTelemetryView>,
    pub tokens_generated: usize,
    pub syscalls_used: usize,
    pub elapsed_secs: f64,
    pub tokens: usize,
    pub max_tokens: usize,
    pub orchestration: Option<WorkspaceOrchestrationSnapshot>,
    pub context: Option<WorkspaceContextSnapshot>,
    pub audit_events: Vec<AuditEvent>,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceOrchestrationSnapshot {
    pub orchestration_id: u64,
    pub task_id: String,
    pub total: usize,
    pub completed: usize,
    pub running: usize,
    pub pending: usize,
    pub failed: usize,
    pub skipped: usize,
    pub finished: bool,
    pub elapsed_secs: f64,
    pub policy: String,
    pub tasks: Vec<WorkspaceOrchestrationTask>,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceOrchestrationTask {
    pub task: String,
    pub status: String,
    pub pid: Option<u64>,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceContextSnapshot {
    pub context_strategy: String,
    pub context_tokens_used: usize,
    pub context_window_size: usize,
    pub context_compressions: u64,
    pub context_retrieval_hits: u64,
    pub last_compaction_reason: Option<String>,
    pub last_summary_ts: Option<String>,
    pub context_segments: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct AuditEvent {
    pub category: String,
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub recorded_at_ms: i64,
    pub session_id: Option<String>,
    pub pid: Option<u64>,
    pub runtime_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct StartSessionResult {
    pub session_id: String,
    pub pid: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct TimelineSnapshot {
    pub session_id: String,
    pub pid: u64,
    pub running: bool,
    pub workload: String,
    pub source: String,
    pub fallback_notice: Option<String>,
    pub error: Option<String>,
    pub items: Vec<TimelineItem>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum TimelineItemKind {
    UserMessage,
    Thinking,
    ToolCall,
    ActionCall,
    ToolResult,
    AssistantMessage,
    SystemEvent,
}

#[derive(Debug, Serialize, Clone)]
pub struct TimelineItem {
    pub id: String,
    pub kind: TimelineItemKind,
    pub text: String,
    pub status: String,
}
