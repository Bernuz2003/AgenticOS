use agentic_control_models::{AssistantSegmentKind, BackendCapabilitiesView, DiagnosticEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::checkpoint::MemoryCountersSnapshot;
use crate::process::{ContextPolicy, ContextState, HumanInputRequest};
use crate::tools::invocation::ProcessPermissionPolicy;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AgentCoreDumpManifest {
    pub format: String,
    pub dump_id: String,
    pub created_at_ms: i64,
    pub capture: CoreDumpCaptureMetadata,
    pub target: CoreDumpTargetMetadata,
    #[serde(default)]
    pub session: Option<CoreDumpSessionMetadata>,
    #[serde(default)]
    pub runtime: Option<CoreDumpRuntimeMetadata>,
    #[serde(default)]
    pub process: Option<CoreDumpProcessMetadata>,
    #[serde(default)]
    pub turn_assembly: Option<CoreDumpTurnAssembly>,
    #[serde(default)]
    pub replay_messages: Vec<CoreDumpReplayMessage>,
    #[serde(default)]
    pub session_audit_events: Vec<DiagnosticEvent>,
    #[serde(default)]
    pub tool_audit_lines: Vec<Value>,
    #[serde(default)]
    pub debug_checkpoints: Vec<CoreDumpDebugCheckpoint>,
    #[serde(default)]
    pub tool_invocation_history: Vec<CoreDumpToolInvocation>,
    pub memory: MemoryCountersSnapshot,
    #[serde(default)]
    pub workspace: Option<WorkspaceSnapshot>,
    #[serde(default)]
    pub backend_state: Option<CoreDumpAvailability>,
    #[serde(default)]
    pub logprobs: Option<CoreDumpAvailability>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CoreDumpCaptureMetadata {
    pub mode: String,
    pub reason: String,
    #[serde(default)]
    pub note: Option<String>,
    pub fidelity: String,
    pub freeze_requested: bool,
    pub freeze_applied: bool,
    pub include_workspace: bool,
    pub include_backend_state: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CoreDumpTargetMetadata {
    pub source: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub pid: Option<u64>,
    #[serde(default)]
    pub runtime_id: Option<String>,
    pub in_flight: bool,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CoreDumpSessionMetadata {
    pub session_id: String,
    pub title: String,
    pub state: String,
    #[serde(default)]
    pub active_pid: Option<u64>,
    #[serde(default)]
    pub runtime_id: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CoreDumpRuntimeMetadata {
    pub runtime_id: String,
    pub target_kind: String,
    pub logical_model_id: String,
    pub display_path: String,
    pub backend_id: String,
    pub backend_class: String,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub remote_model_id: Option<String>,
    pub load_mode: String,
    pub reservation_ram_bytes: u64,
    pub reservation_vram_bytes: u64,
    pub pinned: bool,
    #[serde(default)]
    pub transition_state: Option<String>,
    pub loaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CoreDumpProcessMetadata {
    pub owner_id: usize,
    pub tool_caller: String,
    pub permission_policy: ProcessPermissionPolicy,
    pub token_count: usize,
    pub tokens: Vec<u32>,
    pub index_pos: usize,
    pub turn_start_index: usize,
    pub max_tokens: usize,
    #[serde(default)]
    pub context_slot_id: Option<u64>,
    #[serde(default)]
    pub resident_slot_policy: Option<String>,
    #[serde(default)]
    pub resident_slot_state: Option<String>,
    #[serde(default)]
    pub resident_slot_snapshot_path: Option<String>,
    #[serde(default)]
    pub backend_id: Option<String>,
    #[serde(default)]
    pub backend_class: Option<String>,
    #[serde(default)]
    pub backend_capabilities: Option<BackendCapabilitiesView>,
    pub prompt_text: String,
    pub resident_prompt_checkpoint_bytes: usize,
    pub rendered_inference_prompt: String,
    pub resident_prompt_suffix: String,
    #[serde(default)]
    pub generation: Option<CoreDumpGenerationMetadata>,
    pub context_policy: ContextPolicy,
    pub context_state: ContextState,
    #[serde(default)]
    pub pending_human_request: Option<HumanInputRequest>,
    #[serde(default)]
    pub termination_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CoreDumpGenerationMetadata {
    pub temperature: f64,
    pub top_p: f64,
    pub seed: u64,
    pub max_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CoreDumpTurnAssembly {
    pub raw_transport_text: String,
    pub visible_projection: String,
    pub thinking_projection: String,
    #[serde(default)]
    pub pending_invocation: Option<String>,
    #[serde(default)]
    pub pending_segments: Vec<CoreDumpAssistantSegment>,
    pub output_stop_requested: bool,
    pub generated_token_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CoreDumpAssistantSegment {
    pub kind: AssistantSegmentKind,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CoreDumpReplayMessage {
    pub role: String,
    pub kind: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CoreDumpDebugCheckpoint {
    pub checkpoint_id: i64,
    pub recorded_at_ms: i64,
    pub boundary: String,
    pub state: String,
    pub snapshot: DebugCheckpointSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DebugCheckpointSnapshot {
    pub prompt_text: String,
    pub resident_prompt_checkpoint_bytes: usize,
    pub rendered_inference_prompt: String,
    pub context_policy: ContextPolicy,
    pub context_state: ContextState,
    #[serde(default)]
    pub pending_human_request: Option<HumanInputRequest>,
    #[serde(default)]
    pub termination_reason: Option<String>,
    #[serde(default)]
    pub turn_assembly: Option<CoreDumpTurnAssembly>,
    #[serde(default)]
    pub invocation: Option<DebugCheckpointInvocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DebugCheckpointInvocation {
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CoreDumpToolInvocation {
    pub tool_call_id: String,
    pub recorded_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub pid: Option<u64>,
    #[serde(default)]
    pub runtime_id: Option<String>,
    pub tool_name: String,
    pub caller: String,
    pub transport: String,
    pub status: String,
    pub command_text: String,
    pub input: Value,
    #[serde(default)]
    pub output: Option<Value>,
    #[serde(default)]
    pub output_text: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub error_kind: Option<String>,
    #[serde(default)]
    pub error_text: Option<String>,
    #[serde(default)]
    pub effects: Vec<Value>,
    #[serde(default)]
    pub duration_ms: Option<u128>,
    pub kill: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CoreDumpAvailability {
    pub available: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkspaceEntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WorkspaceSnapshot {
    pub root: String,
    #[serde(default)]
    pub skipped_roots: Vec<String>,
    pub total_entries: usize,
    pub total_bytes: u64,
    pub truncated: bool,
    #[serde(default)]
    pub entries: Vec<WorkspaceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WorkspaceEntry {
    pub path: String,
    pub kind: WorkspaceEntryKind,
    #[serde(default)]
    pub bytes: Option<u64>,
    #[serde(default)]
    pub modified_at_ms: Option<u128>,
}
