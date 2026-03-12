use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::backend::BackendCapabilities;
use crate::memory::ContextSlotId;
use crate::process::{ResidentSlotPolicy, ResidentSlotState};

#[derive(Debug, Clone)]
struct ResidentSlotLease {
    #[allow(dead_code)]
    slot_id: ContextSlotId,
    policy: ResidentSlotPolicy,
    state: ResidentSlotState,
    snapshot_path: Option<PathBuf>,
}

pub(crate) struct ResidentSlotManager {
    leases: HashMap<u64, ResidentSlotLease>,
}

impl ResidentSlotManager {
    pub(super) fn new() -> Self {
        Self {
            leases: HashMap::new(),
        }
    }

    pub(super) fn policy_for_capabilities(capabilities: BackendCapabilities) -> ResidentSlotPolicy {
        if capabilities.persistent_slots && capabilities.save_restore_slots {
            ResidentSlotPolicy::ParkAndResume
        } else {
            ResidentSlotPolicy::Unmanaged
        }
    }

    pub(super) fn bind(
        &mut self,
        pid: u64,
        #[allow(dead_code)] slot_id: ContextSlotId,
        policy: ResidentSlotPolicy,
    ) -> ResidentSlotPolicy {
        self.leases.insert(
            pid,
            ResidentSlotLease {
                slot_id,
                policy,
                state: ResidentSlotState::Allocated,
                snapshot_path: None,
            },
        );
        policy
    }

    pub(super) fn mark_park_requested(&mut self, pid: u64) -> Option<ResidentSlotPolicy> {
        let lease = self.leases.get_mut(&pid)?;
        lease.state = ResidentSlotState::ParkRequested;
        Some(lease.policy)
    }

    pub(super) fn mark_snapshot_saved(
        &mut self,
        pid: u64,
        path: &Path,
    ) -> Option<ResidentSlotPolicy> {
        let lease = self.leases.get_mut(&pid)?;
        lease.state = ResidentSlotState::SnapshotSaved;
        lease.snapshot_path = Some(path.to_path_buf());
        Some(lease.policy)
    }

    pub(super) fn mark_restoring(&mut self, pid: u64, path: &Path) -> Option<ResidentSlotPolicy> {
        let lease = self.leases.get_mut(&pid)?;
        lease.state = ResidentSlotState::Restoring;
        lease.snapshot_path = Some(path.to_path_buf());
        Some(lease.policy)
    }

    pub(super) fn mark_allocated(&mut self, pid: u64) -> Option<ResidentSlotPolicy> {
        let lease = self.leases.get_mut(&pid)?;
        lease.state = ResidentSlotState::Allocated;
        Some(lease.policy)
    }

    pub(super) fn remove(&mut self, pid: u64) {
        self.leases.remove(&pid);
    }

    #[cfg(test)]
    pub(crate) fn lease_for(
        &self,
        pid: u64,
    ) -> Option<(ContextSlotId, ResidentSlotPolicy, ResidentSlotState)> {
        self.leases
            .get(&pid)
            .map(|lease| (lease.slot_id, lease.policy, lease.state))
    }
}
