use std::collections::HashMap;
use std::path::PathBuf;

use super::swap::{SwapCounterDeltas, SwapManager};
use super::types::{ContextSlotId, ResidencySnapshot, SwapEvent};
use crate::errors::MemoryError;
use crate::prompting::PromptFamily;

#[derive(Debug, Clone)]
struct LogicalSlotRecord {
    owner_pid: Option<u64>,
    token_slots: usize,
}

pub(super) struct LogicalResidencyManager {
    slots: HashMap<ContextSlotId, LogicalSlotRecord>,
    pid_to_slot: HashMap<u64, ContextSlotId>,
    token_slot_quota_per_pid: usize,
    next_slot_id: ContextSlotId,
    swap: SwapManager,
}

impl LogicalResidencyManager {
    pub(super) fn new(token_slot_quota_per_pid: usize) -> Self {
        Self {
            slots: HashMap::new(),
            pid_to_slot: HashMap::new(),
            token_slot_quota_per_pid: token_slot_quota_per_pid.max(1),
            next_slot_id: 1,
            swap: SwapManager::new(),
        }
    }

    pub(super) fn configure_async_swap(
        &mut self,
        enabled: bool,
        swap_dir: Option<PathBuf>,
    ) -> Result<(), MemoryError> {
        self.swap.configure(enabled, swap_dir)
    }

    pub(super) fn set_token_slot_quota_per_pid(&mut self, quota: usize) {
        self.token_slot_quota_per_pid = quota.max(1);
    }

    pub(super) fn register_process(
        &mut self,
        pid: u64,
        token_slots: usize,
    ) -> Result<ContextSlotId, MemoryError> {
        if token_slots == 0 {
            return Err(MemoryError::ZeroTokenSlots);
        }
        if token_slots > self.token_slot_quota_per_pid {
            return Err(MemoryError::QuotaExceeded {
                pid,
                requested: token_slots,
                quota: self.token_slot_quota_per_pid,
            });
        }

        if let Some(existing) = self.pid_to_slot.get(&pid).copied() {
            if let Some(slot) = self.slots.get_mut(&existing) {
                slot.owner_pid = Some(pid);
                slot.token_slots = token_slots;
            }
            return Ok(existing);
        }

        Ok(self.alloc_slot(Some(pid), token_slots))
    }

    pub(super) fn release_process(&mut self, pid: u64) -> Option<ContextSlotId> {
        self.swap.remove_waiting(pid);
        let slot_id = self.pid_to_slot.remove(&pid)?;
        self.slots.remove(&slot_id);
        Some(slot_id)
    }

    pub(super) fn logical_slot_count(&self) -> usize {
        self.slots.len()
    }

    pub(super) fn tracked_pids(&self) -> usize {
        self.pid_to_slot.len()
    }

    pub(super) fn slot_for_pid(&self, pid: u64) -> Option<ContextSlotId> {
        self.pid_to_slot.get(&pid).copied()
    }

    pub(super) fn is_pid_parked(&self, pid: u64) -> bool {
        self.swap.is_pid_waiting(pid)
    }

    pub(super) fn clear_parked(&mut self, pid: u64) {
        self.swap.remove_waiting(pid);
    }

    pub(super) fn swap_enabled(&self) -> bool {
        self.swap.is_enabled()
    }

    pub(super) fn enqueue_swap(
        &mut self,
        pid: u64,
        backend_id: &str,
        family: PromptFamily,
        pressure_bytes: usize,
    ) -> Result<String, MemoryError> {
        let slot_id = self
            .slot_for_pid(pid)
            .ok_or(MemoryError::PidNotRegistered(pid))?;
        self.swap
            .enqueue(pid, slot_id, backend_id, family, pressure_bytes)
    }

    pub(super) fn poll_swap_events(&mut self) -> (Vec<SwapEvent>, SwapCounterDeltas) {
        self.swap.poll_events()
    }

    pub(super) fn snapshot(&self) -> ResidencySnapshot {
        ResidencySnapshot {
            tracked_pids: self.tracked_pids(),
            logical_slots: self.logical_slot_count(),
            pending_swaps: self.swap.waiting_count(),
            parked_pids: self.swap.waiting_count(),
            swap_worker_crashes: self.swap.worker_crashes(),
        }
    }

    fn alloc_slot(&mut self, owner_pid: Option<u64>, token_slots: usize) -> ContextSlotId {
        let slot_id = self.next_slot_id;
        self.next_slot_id += 1;
        self.slots.insert(
            slot_id,
            LogicalSlotRecord {
                owner_pid,
                token_slots,
            },
        );

        if let Some(pid) = owner_pid {
            self.pid_to_slot.insert(pid, slot_id);
        }

        slot_id
    }
}
