use crate::runtimes::RuntimeReservation;
use crate::storage::StorageError;
/// State tracking and errors for the resource governor.
use thiserror::Error;

#[derive(Debug, Clone)]
pub(crate) struct ResourceGovernorStatus {
    pub(crate) ram_budget_bytes: u64,
    pub(crate) vram_budget_bytes: u64,
    pub(crate) min_ram_headroom_bytes: u64,
    pub(crate) min_vram_headroom_bytes: u64,
    pub(crate) ram_used_bytes: u64,
    pub(crate) vram_used_bytes: u64,
    pub(crate) ram_available_bytes: u64,
    pub(crate) vram_available_bytes: u64,
    pub(crate) pending_queue_depth: usize,
    pub(crate) loader_busy: bool,
    pub(crate) loader_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeLoadQueueEntry {
    pub(crate) queue_id: i64,
    pub(crate) logical_model_id: String,
    pub(crate) display_path: String,
    pub(crate) backend_class: String,
    pub(crate) state: String,
    pub(crate) reservation_ram_bytes: u64,
    pub(crate) reservation_vram_bytes: u64,
    pub(crate) reason: String,
    pub(crate) requested_at_ms: i64,
    pub(crate) updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct AdmissionPlan {
    pub(crate) reservation: RuntimeReservation,
    pub(crate) evict_runtime_ids: Vec<String>,
    pub(crate) requires_loader_lock: bool,
}

#[derive(Debug, Error)]
pub(crate) enum ResourceGovernorError {
    #[error("{0}")]
    Storage(#[from] StorageError),

    #[error("{0}")]
    Busy(String),

    #[error("{0}")]
    Refused(String),
}
