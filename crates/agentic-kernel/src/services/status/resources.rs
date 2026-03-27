use std::collections::HashSet;

use agentic_control_models::ResourceGovernorStatusView;

use super::view::StatusSnapshotDeps;
use crate::runtimes::build_runtime_load_queue_views;

pub(super) fn build_resource_governor_view(
    deps: &StatusSnapshotDeps<'_>,
) -> ResourceGovernorStatusView {
    let governor_status = deps.resource_governor.status(deps.runtime_registry);
    ResourceGovernorStatusView {
        ram_budget_bytes: governor_status.ram_budget_bytes,
        vram_budget_bytes: governor_status.vram_budget_bytes,
        min_ram_headroom_bytes: governor_status.min_ram_headroom_bytes,
        min_vram_headroom_bytes: governor_status.min_vram_headroom_bytes,
        ram_used_bytes: governor_status.ram_used_bytes,
        vram_used_bytes: governor_status.vram_used_bytes,
        ram_available_bytes: governor_status.ram_available_bytes,
        vram_available_bytes: governor_status.vram_available_bytes,
        pending_queue_depth: governor_status.pending_queue_depth,
        loader_busy: governor_status.loader_busy,
        loader_reason: governor_status.loader_reason,
    }
}

pub(super) fn build_runtime_load_queue(
    deps: &StatusSnapshotDeps<'_>,
) -> Vec<agentic_control_models::RuntimeLoadQueueEntryView> {
    build_runtime_load_queue_views(deps.resource_governor.queue_views())
}

pub(super) fn collect_unique_pids<const N: usize>(groups: [&[u64]; N]) -> Vec<u64> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();

    for group in groups {
        for &pid in group {
            if seen.insert(pid) {
                unique.push(pid);
            }
        }
    }

    unique
}
