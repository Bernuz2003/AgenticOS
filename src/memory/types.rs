/// Shared types for the NeuralMemory subsystem.
pub type TensorId = u64;

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
    pub waiting_pids: usize,
    pub oom_events: u64,
}

#[derive(Debug, Clone)]
pub struct SwapEvent {
    pub pid: u64,
    pub success: bool,
    pub detail: String,
}

#[derive(Clone)]
pub struct MemoryConfig {
    pub block_size: usize,
    pub hidden_dim: usize,
    pub total_memory_mb: usize,
}
