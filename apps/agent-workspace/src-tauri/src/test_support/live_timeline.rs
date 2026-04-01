use agentic_control_models::KernelEvent;

pub use crate::kernel::live_timeline::{
    TimelineSeedMessage, TimelineSeedSession, TimelineSeedTurn, TimelineStore,
};
pub use crate::models::kernel::{TimelineItem, TimelineItemKind, TimelineSnapshot};

pub fn apply_kernel_event(store: &mut TimelineStore, event: &KernelEvent) {
    crate::kernel::events::apply_timeline_store_event(store, event);
}

pub fn finish_session_with_reason(store: &mut TimelineStore, pid: u64, reason: Option<&str>) {
    store.finish_session_with_reason(pid, None, reason);
}
