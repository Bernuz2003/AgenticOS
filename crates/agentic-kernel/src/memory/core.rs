use std::path::{Path, PathBuf};

use super::residency::LogicalResidencyManager;
use super::types::{ContextSlotId, MemorySnapshot, SlotPersistenceKind, SwapEvent};
use crate::errors::MemoryError;

#[derive(Default)]
pub(super) struct MemoryCounters {
    pub(super) swap_count: u64,
    pub(super) swap_faults: u64,
    pub(super) swap_failures: u64,
    pub(super) oom_events: u64,
}

pub struct NeuralMemory {
    active: bool,
    residency: LogicalResidencyManager,
    pub(super) counters: MemoryCounters,
}

impl NeuralMemory {
    pub fn new() -> Result<Self, MemoryError> {
        tracing::info!("NeuralMemory: init (logical residency + resident slot parking)");

        Ok(Self {
            active: true,
            residency: LogicalResidencyManager::new(
                crate::config::kernel_config()
                    .memory
                    .token_slot_quota_per_pid,
            ),
            counters: MemoryCounters::default(),
        })
    }

    pub fn configure_async_swap(
        &mut self,
        enabled: bool,
        swap_dir: Option<PathBuf>,
    ) -> Result<(), MemoryError> {
        self.residency.configure_async_swap(enabled, swap_dir)
    }

    #[cfg(test)]
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn set_token_slot_quota_per_pid(&mut self, quota: usize) {
        self.residency.set_token_slot_quota_per_pid(quota);
    }

    pub fn register_process(
        &mut self,
        pid: u64,
        token_slots: usize,
    ) -> Result<ContextSlotId, MemoryError> {
        match self.residency.register_process(pid, token_slots) {
            Ok(slot_id) => Ok(slot_id),
            Err(err @ (MemoryError::ZeroTokenSlots | MemoryError::QuotaExceeded { .. })) => {
                self.counters.oom_events += 1;
                Err(err)
            }
            Err(err) => Err(err),
        }
    }

    pub fn release_process(&mut self, pid: u64) -> Result<String, MemoryError> {
        let Some(slot_id) = self.residency.release_process(pid) else {
            return Ok(format!("NeuralMemory: PID {} had no allocation", pid));
        };

        Ok(format!("Released logical slot {} (pid={})", slot_id, pid))
    }

    #[allow(dead_code)]
    pub fn write_for_pid_bytes(
        &mut self,
        pid: u64,
        raw_data: &[u8],
    ) -> Result<String, MemoryError> {
        self.write_for_pid_bytes_with_backend(pid, raw_data, None)
    }

    pub fn write_for_pid_bytes_with_backend(
        &mut self,
        pid: u64,
        raw_data: &[u8],
        backend_id: Option<&str>,
    ) -> Result<String, MemoryError> {
        let Some(slot_id) = self.residency.slot_for_pid(pid) else {
            if !self.active {
                return Ok(format!(
                    "NeuralMemory inactive: MEMW skipped for PID {} ({} bytes)",
                    pid,
                    raw_data.len()
                ));
            }
            return Err(MemoryError::PidNotRegistered(pid));
        };

        if !self.active {
            return Ok(format!(
                "NeuralMemory inactive: MEMW skipped for PID {} ({} bytes)",
                pid,
                raw_data.len()
            ));
        }

        if !self.residency.swap_enabled() {
            self.residency.clear_parked(pid);
            return Ok(format!(
                "Resident memory pressure noted for PID {} slot {} ({} bytes); async parking disabled",
                pid,
                slot_id,
                raw_data.len()
            ));
        }

        let backend_id = backend_id.ok_or_else(|| {
            MemoryError::Swap(
                "Cannot enqueue resident-slot park without an active backend id".to_string(),
            )
        })?;

        self.counters.swap_faults += 1;
        self.residency.enqueue_swap(pid, backend_id, raw_data.len())
    }

    pub fn snapshot(&self) -> MemorySnapshot {
        let residency = self.residency.snapshot();

        MemorySnapshot {
            active: self.active,
            total_blocks: 0,
            free_blocks: 0,
            allocated_tensors: residency.logical_slots,
            tracked_pids: residency.tracked_pids,
            alloc_bytes: 0,
            evictions: 0,
            swap_count: self.counters.swap_count,
            swap_faults: self.counters.swap_faults,
            swap_failures: self.counters.swap_failures,
            pending_swaps: residency.pending_swaps,
            parked_pids: residency.parked_pids,
            oom_events: self.counters.oom_events,
            swap_worker_crashes: residency.swap_worker_crashes,
        }
    }

    pub fn is_pid_parked(&self, pid: u64) -> bool {
        self.residency.is_pid_parked(pid)
    }

    pub fn slot_for_pid(&self, pid: u64) -> Option<ContextSlotId> {
        self.residency.slot_for_pid(pid)
    }

    pub fn poll_swap_events(&mut self) -> Vec<SwapEvent> {
        let (events, deltas) = self.residency.poll_swap_events();
        self.counters.swap_count += deltas.swap_count_inc;
        self.counters.swap_failures += deltas.swap_failures_inc;
        self.counters.swap_faults += deltas.swap_faults_inc;
        events
    }

    pub fn restore_swapped_pid(
        &mut self,
        pid: u64,
        slot_id: ContextSlotId,
        persistence_kind: SlotPersistenceKind,
        swap_path: Option<&Path>,
    ) -> Result<String, MemoryError> {
        match persistence_kind {
            SlotPersistenceKind::BackendSlotSnapshot => {
                let Some(path) = swap_path else {
                    return Err(MemoryError::Swap(format!(
                        "resident backend slot restore for PID {} had no snapshot path",
                        pid
                    )));
                };

                Ok(format!(
                    "resident backend slot snapshot ready pid={} slot={} snapshot={}",
                    pid,
                    slot_id,
                    path.display()
                ))
            }
            SlotPersistenceKind::Unknown => Err(MemoryError::Swap(format!(
                "swap completion for PID {} used unknown persistence kind",
                pid
            ))),
        }
    }
}

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
