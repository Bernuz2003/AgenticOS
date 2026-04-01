use std::sync::atomic::{AtomicU64, Ordering};

static TOOL_CALL_COUNTER: AtomicU64 = AtomicU64::new(1);
static ACTION_CALL_COUNTER: AtomicU64 = AtomicU64::new(1);
static IPC_MESSAGE_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(super) fn next_tool_call_id(pid: u64) -> String {
    let seq = TOOL_CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("tool-{pid}-{seq}")
}

pub(super) fn next_action_call_id(pid: u64) -> String {
    let seq = ACTION_CALL_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("action-{pid}-{seq}")
}

pub(super) fn next_ipc_message_id() -> String {
    let seq = IPC_MESSAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("ipc-{}-{seq}", crate::storage::current_timestamp_ms())
}
