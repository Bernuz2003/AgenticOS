use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use super::swap::SwapManager;
use super::types::{ContextSlotId, MemoryConfig, MemorySnapshot, SwapEvent};
use crate::errors::MemoryError;

type BlockIndex = usize;

#[derive(Default)]
pub(super) struct MemoryCounters {
    pub(super) alloc_bytes: usize,
    pub(super) evictions: u64,
    pub(super) swap_count: u64,
    pub(super) swap_faults: u64,
    pub(super) swap_failures: u64,
    pub(super) oom_events: u64,
}

pub(super) struct ContextSlotRecord {
    pub(super) pages: Vec<BlockIndex>,
    pub(super) owner_pid: Option<u64>,
}

pub struct NeuralMemory {
    pub(super) config: MemoryConfig,

    total_blocks: usize,
    pub(super) free_blocks: VecDeque<BlockIndex>,
    pub(super) slot_table: HashMap<ContextSlotId, ContextSlotRecord>,

    pub(super) pid_to_slot: HashMap<u64, ContextSlotId>,
    pub(super) pid_token_slots: HashMap<u64, usize>,
    token_slot_quota_per_pid: usize,
    pub(super) counters: MemoryCounters,
    active: bool,

    pub(super) swap: SwapManager,
    pub(super) lru_order: VecDeque<ContextSlotId>,

    next_slot_id: ContextSlotId,
}

impl NeuralMemory {
    pub fn new(config: MemoryConfig) -> Result<Self, MemoryError> {
        let elements_per_block = config.block_size * config.hidden_dim;
        let bytes_per_element = 4;
        let total_bytes = config.total_memory_mb * 1024 * 1024;
        let bytes_per_block = elements_per_block * bytes_per_element;
        let num_blocks = total_bytes / bytes_per_block;

        tracing::info!(
            total_mb = config.total_memory_mb,
            blocks = num_blocks,
            params_per_block = elements_per_block,
            "NeuralMemory: init (logical context slots)"
        );

        let mut free_blocks = VecDeque::with_capacity(num_blocks);

        for i in 0..num_blocks {
            free_blocks.push_back(i);
        }

        Ok(NeuralMemory {
            config,
            total_blocks: num_blocks,
            free_blocks,
            slot_table: HashMap::new(),
            pid_to_slot: HashMap::new(),
            pid_token_slots: HashMap::new(),
            token_slot_quota_per_pid: 4096,
            counters: MemoryCounters::default(),
            active: true,
            swap: SwapManager::new(),
            lru_order: VecDeque::new(),
            next_slot_id: 1,
        })
    }

    pub fn configure_async_swap(
        &mut self,
        enabled: bool,
        swap_dir: Option<PathBuf>,
    ) -> Result<(), MemoryError> {
        self.swap.configure(enabled, swap_dir)
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.active
    }

    #[cfg(test)]
    pub fn set_token_slot_quota_per_pid(&mut self, quota: usize) {
        self.token_slot_quota_per_pid = quota.max(1);
    }

    pub fn register_process(&mut self, pid: u64, token_slots: usize) -> Result<ContextSlotId, MemoryError> {
        if !self.active {
            return Ok(0);
        }

        if token_slots == 0 {
            self.counters.oom_events += 1;
            return Err(MemoryError::ZeroTokenSlots);
        }
        if token_slots > self.token_slot_quota_per_pid {
            self.counters.oom_events += 1;
            return Err(MemoryError::QuotaExceeded {
                pid,
                requested: token_slots,
                quota: self.token_slot_quota_per_pid,
            });
        }

        if let Some(existing) = self.pid_to_slot.get(&pid).copied() {
            self.pid_token_slots.insert(pid, token_slots);
            self.touch_slot_lru(existing);
            return Ok(existing);
        }

        let slot_id = self.alloc_slot();
        self.pid_to_slot.insert(pid, slot_id);
        if let Some(slot) = self.slot_table.get_mut(&slot_id) {
            slot.owner_pid = Some(pid);
        }
        self.pid_token_slots.insert(pid, token_slots);
        Ok(slot_id)
    }

    pub fn release_process(&mut self, pid: u64) -> Result<String, MemoryError> {
        if !self.active {
            return Ok(format!("NeuralMemory disabled: release skipped for PID {}", pid));
        }

        self.swap.remove_waiting(pid);

        let Some(slot_id) = self.pid_to_slot.remove(&pid) else {
            return Ok(format!("NeuralMemory: PID {} had no allocation", pid));
        };

        self.pid_token_slots.remove(&pid);
        self.release_slot(slot_id)
    }

    #[allow(dead_code)]
    pub fn write_for_pid_bytes(&mut self, pid: u64, raw_data: &[u8]) -> Result<String, MemoryError> {
        self.write_for_pid_bytes_with_backend(pid, raw_data, None)
    }

    pub fn write_for_pid_bytes_with_backend(
        &mut self,
        pid: u64,
        raw_data: &[u8],
        backend_id: Option<&str>,
    ) -> Result<String, MemoryError> {
        if !self.active {
            return Ok(format!(
                "NeuralMemory disabled: MEMW skipped for PID {} ({} bytes)",
                pid,
                raw_data.len()
            ));
        }

        let slot_id = self
            .pid_to_slot
            .get(&pid)
            .copied()
            .ok_or(MemoryError::PidNotRegistered(pid))?;
        match self.write_slot_bytes(slot_id, raw_data) {
            Ok(msg) => {
                self.swap.remove_waiting(pid);
                Ok(msg)
            }
            Err(e) => {
                if matches!(e, MemoryError::OutOfMemory { .. }) && self.swap.is_enabled() {
                    let backend_id = backend_id.ok_or_else(|| {
                        MemoryError::Swap(
                            "Cannot enqueue backend-owned swap without an active backend id"
                                .to_string(),
                        )
                    })?;
                    self.counters.swap_faults += 1;
                    let queued = self
                        .swap
                        .enqueue(pid, slot_id, backend_id, raw_data.to_vec())?;
                    return Ok(queued);
                }
                Err(e)
            }
        }
    }

    pub fn snapshot(&self) -> MemorySnapshot {
        MemorySnapshot {
            active: self.active,
            total_blocks: self.total_blocks,
            free_blocks: self.free_blocks.len(),
            allocated_tensors: self.slot_table.len(),
            tracked_pids: self.pid_to_slot.len(),
            alloc_bytes: self.counters.alloc_bytes,
            evictions: self.counters.evictions,
            swap_count: self.counters.swap_count,
            swap_faults: self.counters.swap_faults,
            swap_failures: self.counters.swap_failures,
            pending_swaps: self.swap.waiting_count(),
            waiting_pids: self.swap.waiting_count(),
            oom_events: self.counters.oom_events,
            swap_worker_crashes: self.swap.worker_crashes(),
        }
    }

    pub fn is_pid_waiting_for_memory(&self, pid: u64) -> bool {
        self.swap.is_pid_waiting(pid)
    }

    pub fn slot_for_pid(&self, pid: u64) -> Option<ContextSlotId> {
        self.pid_to_slot.get(&pid).copied()
    }

    pub fn poll_swap_events(&mut self) -> Vec<SwapEvent> {
        let (events, deltas) = self.swap.poll_events();
        self.counters.swap_count += deltas.swap_count_inc;
        self.counters.swap_failures += deltas.swap_failures_inc;
        self.counters.swap_faults += deltas.swap_faults_inc;
        events
    }

    pub fn alloc_slot(&mut self) -> ContextSlotId {
        let id = self.next_slot_id;
        self.next_slot_id += 1;
        self.slot_table.insert(
            id,
            ContextSlotRecord {
                pages: Vec::new(),
                owner_pid: None,
            },
        );
        self.touch_slot_lru(id);
        id
    }

    pub fn write_slot_bytes(&mut self, slot_id: ContextSlotId, raw_data: &[u8]) -> Result<String, MemoryError> {
        if !self.active {
            return Ok(format!(
                "NeuralMemory disabled: write skipped for slot {} ({} bytes)",
                slot_id,
                raw_data.len()
            ));
        }

        if !self.slot_table.contains_key(&slot_id) {
            return Err(MemoryError::TensorNotFound(slot_id));
        }

        if raw_data.len() % 4 != 0 {
            return Err(MemoryError::MisalignedPayload {
                bytes: raw_data.len(),
            });
        }

        self.clear_slot_pages(slot_id, true);

        let element_count = raw_data.len() / 4;

        if element_count == 0 {
            return Ok("No data".to_string());
        }

        let elements_per_block = self.config.block_size * self.config.hidden_dim;
        let blocks_needed = element_count.div_ceil(elements_per_block);

        if self.free_blocks.len() < blocks_needed {
            let recovered = self.evict_lru_until_fit(blocks_needed, Some(slot_id));
            if !recovered {
                self.counters.oom_events += 1;
                return Err(MemoryError::OutOfMemory { detail: "Not enough GPU blocks".into() });
            }
        }

        if self.free_blocks.len() < blocks_needed {
            self.counters.oom_events += 1;
            return Err(MemoryError::OutOfMemory { detail: "Not enough GPU blocks".into() });
        }

        for _ in 0..blocks_needed {
            let block_idx = self.free_blocks.pop_front().unwrap();
            if let Some(slot) = self.slot_table.get_mut(&slot_id) {
                slot.pages.push(block_idx);
            }
        }

        self.counters.alloc_bytes += blocks_needed * elements_per_block * 4;
        self.touch_slot_lru(slot_id);

        Ok(format!(
            "Written {} floats into slot {} across {} blocks",
            element_count,
            slot_id,
            blocks_needed
        ))
    }

    pub fn release_slot(&mut self, slot_id: ContextSlotId) -> Result<String, MemoryError> {
        if !self.active {
            return Ok(format!("NeuralMemory disabled: release skipped for slot {}", slot_id));
        }

        let slot = self
            .slot_table
            .remove(&slot_id)
            .ok_or(MemoryError::TensorNotFound(slot_id))?;
        let pages = slot.pages;

        let elements_per_block = self.config.block_size * self.config.hidden_dim;
        let released_blocks = pages.len();
        for block in pages {
            self.free_blocks.push_back(block);
        }

        let released_bytes = released_blocks * elements_per_block * 4;
        self.counters.alloc_bytes = self.counters.alloc_bytes.saturating_sub(released_bytes);

        if let Some(pid) = slot.owner_pid {
            self.pid_to_slot.remove(&pid);
            self.pid_token_slots.remove(&pid);
        }

        self.lru_order.retain(|&current| current != slot_id);

        Ok(format!("Released slot {} ({} blocks)", slot_id, released_blocks))
    }

    #[allow(dead_code)]
    pub fn alloc(&mut self) -> ContextSlotId {
        self.alloc_slot()
    }

    #[allow(dead_code)]
    pub fn write_from_bytes(&mut self, id: ContextSlotId, raw_data: &[u8]) -> Result<String, MemoryError> {
        self.write_slot_bytes(id, raw_data)
    }

    #[allow(dead_code)]
    pub fn release_tensor(&mut self, id: ContextSlotId) -> Result<String, MemoryError> {
        self.release_slot(id)
    }


}

#[cfg(test)]
mod tests {
    use crate::memory::{MemoryConfig, NeuralMemory};
    use std::fs;
    use std::path::PathBuf;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn register_write_release_pid_flow() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 4,
            hidden_dim: 4,
            total_memory_mb: 1,
        })
        .expect("memory init");

        mem.set_token_slot_quota_per_pid(32);
        let _slot = mem.register_process(42, 16).expect("register pid");

        let payload = [1.0f32, 2.0f32, 3.0f32, 4.0f32]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();
        let write_msg = mem
            .write_for_pid_bytes(42, &payload)
            .expect("write bytes for pid");
        assert!(write_msg.contains("Written"));

        let snapshot_before_release = mem.snapshot();
        assert_eq!(snapshot_before_release.tracked_pids, 1);
        assert!(snapshot_before_release.alloc_bytes > 0);

        let rel = mem.release_process(42).expect("release pid");
        assert!(rel.contains("Released slot") || rel.contains("had no allocation"));

        let snapshot_after_release = mem.snapshot();
        assert_eq!(snapshot_after_release.tracked_pids, 0);
    }

    #[test]
    fn write_for_pid_bytes_rejects_misaligned_payload() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 4,
            hidden_dim: 4,
            total_memory_mb: 1,
        })
        .expect("memory init");

        mem.set_token_slot_quota_per_pid(32);
        mem.register_process(9, 16).expect("register pid");

        let err = mem
            .write_for_pid_bytes(9, b"12345")
            .expect_err("misaligned payload should fail");
        assert!(err.to_string().contains("not aligned to 4 bytes"));
    }

    #[test]
    fn quota_enforcement_increments_oom_counter() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 4,
            hidden_dim: 4,
            total_memory_mb: 1,
        })
        .expect("memory init");

        mem.set_token_slot_quota_per_pid(8);
        let err = mem
            .register_process(7, 64)
            .expect_err("quota should reject large token slots");
        assert!(err.to_string().contains("quota"));
        assert!(mem.snapshot().oom_events >= 1);
    }

    #[test]
    fn fallback_mode_is_noop_and_safe() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 4,
            hidden_dim: 4,
            total_memory_mb: 1,
        })
        .expect("memory init");

        mem.set_active(false);
        assert!(!mem.is_active());

        let reg = mem.register_process(100, 12).expect("register noop");
        assert_eq!(reg, 0);

        let wr = mem
            .write_for_pid_bytes(100, b"hello")
            .expect("write noop should be ok");
        assert!(wr.contains("disabled"));

        let rel = mem.release_process(100).expect("release noop");
        assert!(rel.contains("disabled"));
    }

    #[test]
    fn pressure_with_multiple_pids_triggers_oom_events() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 1024,
            hidden_dim: 1024,
            total_memory_mb: 1,
        })
        .expect("memory init");
        mem.set_token_slot_quota_per_pid(4096);

        let float_count = 300_000usize;
        let payload = vec![0f32; float_count]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();

        for pid in 1..=3u64 {
            let _ = mem.register_process(pid, 512).expect("register pid");
            let _ = mem.write_for_pid_bytes(pid, &payload);
        }

        let snap = mem.snapshot();
        assert!(snap.oom_events >= 1);
    }

    #[test]
    fn lru_eviction_frees_other_tensor_before_oom() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 512,
            hidden_dim: 256,
            total_memory_mb: 1,
        })
        .expect("memory init");

        mem.set_token_slot_quota_per_pid(4096);
        mem.register_process(1, 512).expect("register pid1");
        mem.register_process(2, 512).expect("register pid2");

        let one_block_floats = 512 * 256;
        let one_block_payload = vec![1.0f32; one_block_floats]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();

        mem.write_for_pid_bytes(1, &one_block_payload)
            .expect("write pid1");
        mem.write_for_pid_bytes(2, &one_block_payload)
            .expect("write pid2");

        let two_block_payload = vec![2.0f32; one_block_floats * 2]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();
        mem.write_for_pid_bytes(1, &two_block_payload)
            .expect("lru eviction should avoid oom");

        let slot2 = mem.pid_to_slot.get(&2).copied().expect("slot pid2");
        let pages2 = mem
            .slot_table
            .get(&slot2)
            .map(|slot| slot.pages.clone())
            .unwrap_or_default();
        assert!(pages2.is_empty(), "pid2 slot should be evicted by LRU");
        assert!(mem.snapshot().evictions >= 2);
    }

    #[test]
    fn async_swap_queue_marks_waiting_and_completes() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 1024,
            hidden_dim: 1024,
            total_memory_mb: 1,
        })
        .expect("memory init");

        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let swap_dir = PathBuf::from(format!("workspace/test_swap_{}", now_ns));
        mem.configure_async_swap(true, Some(swap_dir.clone()))
            .expect("enable async swap");

        mem.set_token_slot_quota_per_pid(4096);
        mem.register_process(1, 512).expect("register pid");

        let float_count = 300_000usize;
        let payload = vec![1.0f32; float_count]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>();

        let msg = mem
            .write_for_pid_bytes_with_backend(1, &payload, Some("candle.quantized_llama"))
            .expect("should enqueue on oom");
        assert!(msg.contains("queued for async swap"));
        assert!(msg.contains("slot"));
        assert!(mem.is_pid_waiting_for_memory(1));
        assert!(mem.snapshot().swap_faults >= 1);

        let mut completed = false;
        for _ in 0..50 {
            let events = mem.poll_swap_events();
            if events.iter().any(|ev| ev.pid == 1 && ev.slot_id != 0 && ev.success) {
                completed = true;
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        assert!(completed, "swap event did not complete in time");
        assert!(!mem.is_pid_waiting_for_memory(1));

        let snap = mem.snapshot();
        assert!(snap.swap_count >= 1);

        let _ = std::fs::remove_dir_all(swap_dir);
    }

    #[test]
    fn configure_async_swap_rejects_outside_workspace() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 4,
            hidden_dim: 4,
            total_memory_mb: 1,
        })
        .expect("memory init");

        let outside = std::env::temp_dir().join("agenticos_swap_outside");
        let err = mem
            .configure_async_swap(true, Some(outside))
            .expect_err("outside workspace path must be rejected");
        assert!(err.to_string().contains("inside workspace root"));
    }

    #[test]
    fn configure_async_swap_rejects_relative_traversal() {
        let mut mem = NeuralMemory::new(MemoryConfig {
            block_size: 4,
            hidden_dim: 4,
            total_memory_mb: 1,
        })
        .expect("memory init");

        let err = mem
            .configure_async_swap(true, Some(PathBuf::from("../swap_escape")))
            .expect_err("relative traversal must be rejected");
        assert!(err.to_string().contains("traversal"));
    }

    #[test]
    fn persist_swap_payload_is_atomic_and_cleans_tmp() {
        use crate::memory::swap::SwapManager;

        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base = PathBuf::from(format!("workspace/test_swap_io_{}", now_ns));
        fs::create_dir_all(&base).expect("create base dir");

        let final_path = SwapManager::persist_payload(
            &base,
            7,
            11,
            "candle.quantized_llama",
            b"abc123",
        )
            .expect("persist payload");
        assert!(final_path.exists());

        let tmp_path = final_path.with_extension("tmp");
        assert!(!tmp_path.exists());

        let body = fs::read(&final_path).expect("read final swap file");
        assert_eq!(body, b"abc123");

        let _ = fs::remove_dir_all(base);
    }
}
