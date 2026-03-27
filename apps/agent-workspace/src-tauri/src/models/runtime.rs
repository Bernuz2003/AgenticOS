use serde::Serialize;

use agentic_control_models::{
    BackendCapabilitiesView, BackendTelemetryView, ManagedLocalRuntimeView, MemoryStatus,
    RemoteModelRuntimeView, ResourceGovernorStatusView, RuntimeInstanceView,
    RuntimeLoadQueueEntryView,
};

pub use super::sessions::*;
pub use super::timeline::*;
pub use super::workflows::*;
use super::jobs::ScheduledJobView;

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
    pub managed_local_runtimes: Vec<ManagedLocalRuntimeView>,
    pub resource_governor: Option<ResourceGovernorStatusView>,
    pub runtime_load_queue: Vec<RuntimeLoadQueueEntryView>,
    pub global_audit_events: Vec<AuditEvent>,
    pub scheduled_jobs: Vec<ScheduledJobView>,
    pub orchestrations: Vec<LobbyOrchestrationSummary>,
    pub sessions: Vec<AgentSessionSummary>,
    pub error: Option<String>,
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
