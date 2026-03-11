use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlMessage {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecStartPayload {
    pub pid: u64,
    pub workload: String,
    pub priority: String,
    pub context_strategy: String,
    pub context_window_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrateResult {
    pub orchestration_id: u64,
    pub total_tasks: usize,
    pub spawned: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub uptime_secs: u64,
    pub total_commands: u64,
    pub total_errors: u64,
    pub total_exec_started: u64,
    pub total_signals: u64,
    pub model: ModelStatus,
    pub generation: Option<GenerationStatus>,
    pub memory: MemoryStatus,
    pub scheduler: SchedulerStatus,
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
    pub waiting_pids: usize,
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
pub struct ProcessesStatus {
    pub active_pids: Vec<u64>,
    pub waiting_pids: Vec<u64>,
    pub in_flight_pids: Vec<u64>,
    pub active_processes: Vec<PidStatusResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationsStatus {
    pub active_orchestrations: Vec<OrchSummaryResponse>,
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
pub struct PidStatusResponse {
    pub pid: u64,
    pub owner_id: usize,
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
    pub context: Option<ContextStatusSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextStatusSnapshot {
    pub context_strategy: String,
    pub context_tokens_used: usize,
    pub context_window_size: usize,
    pub context_compressions: u64,
    pub context_retrieval_hits: u64,
    pub last_compaction_reason: Option<String>,
    pub last_summary_ts: Option<String>,
    pub context_segments: usize,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchTaskEntry {
    pub task: String,
    pub status: String,
    pub pid: Option<u64>,
    pub error: Option<String>,
    pub context: Option<ContextStatusSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalogSnapshot {
    pub selected_model_id: Option<String>,
    pub total_models: usize,
    pub models: Vec<ModelCatalogEntry>,
    pub routing_recommendations: Vec<ModelRoutingRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalogEntry {
    pub id: String,
    pub family: String,
    pub architecture: Option<String>,
    pub path: String,
    pub tokenizer_path: Option<String>,
    pub tokenizer_present: bool,
    pub metadata_source: Option<String>,
    pub backend_preference: Option<String>,
    pub resolved_backend: Option<String>,
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
    pub path: String,
    pub tokenizer_path: Option<String>,
    pub tokenizer_present: bool,
    pub metadata_source: Option<String>,
    pub backend_preference: Option<String>,
    pub resolved_backend: Option<String>,
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
    pub backend: String,
    pub driver_source: String,
    pub driver_rationale: String,
    pub path: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KernelEvent {
    LobbyChanged {
        reason: String,
    },
    WorkspaceChanged {
        pid: u64,
        reason: String,
    },
    SessionStarted {
        pid: u64,
        workload: String,
        prompt: String,
    },
    TimelineChunk {
        pid: u64,
        text: String,
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
    ModelChanged {
        selected_model_id: String,
        loaded_model_id: String,
    },
    KernelShutdownRequested,
}
