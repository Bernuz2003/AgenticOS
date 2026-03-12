use std::path::PathBuf;

/// Shared types for the NeuralMemory subsystem.
pub type ContextSlotId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotPersistenceKind {
    Unknown,
    BackendSlotSnapshot,
}

impl SlotPersistenceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::BackendSlotSnapshot => "backend_slot_snapshot",
        }
    }

    pub const fn requires_backend_restore(self) -> bool {
        matches!(self, Self::BackendSlotSnapshot)
    }
}

#[derive(Debug, Clone)]
pub struct MemorySnapshot {
    pub active: bool,
    pub total_blocks: usize,
    pub free_blocks: usize,
    pub allocated_tensors: usize,
    pub tracked_pids: usize,
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

#[derive(Debug, Clone, Default)]
pub struct ResidencySnapshot {
    pub tracked_pids: usize,
    pub logical_slots: usize,
    pub pending_swaps: usize,
    pub parked_pids: usize,
    pub swap_worker_crashes: u64,
}

#[derive(Debug, Clone)]
pub struct SwapEvent {
    pub pid: u64,
    pub slot_id: ContextSlotId,
    pub success: bool,
    pub detail: String,
    pub persistence_kind: SlotPersistenceKind,
    pub swap_path: Option<PathBuf>,
}
