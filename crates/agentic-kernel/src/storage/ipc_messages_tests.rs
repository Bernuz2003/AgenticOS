use super::{IpcMailboxSelector, NewIpcMessage, StorageService};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn mailbox_loads_pid_task_role_and_channel_targets() {
    let dir = make_temp_dir("agenticos_ipc_mailbox");
    let db_path = dir.join("agenticos.db");
    let mut storage = StorageService::open(&db_path).expect("open storage");

    storage
        .record_ipc_message(&NewIpcMessage {
            message_id: "msg-pid".to_string(),
            orchestration_id: Some(9),
            sender_pid: Some(1),
            sender_task_id: Some("planner".to_string()),
            sender_attempt: Some(1),
            receiver_pid: Some(42),
            receiver_task_id: None,
            receiver_attempt: None,
            receiver_role: None,
            message_type: "notification".to_string(),
            channel: None,
            payload_preview: "pid hello".to_string(),
            payload_text: "pid hello".to_string(),
            status: "queued".to_string(),
        })
        .expect("record pid message");
    storage
        .record_ipc_message(&NewIpcMessage {
            message_id: "msg-task".to_string(),
            orchestration_id: Some(9),
            sender_pid: Some(2),
            sender_task_id: Some("planner".to_string()),
            sender_attempt: Some(1),
            receiver_pid: None,
            receiver_task_id: Some("draft".to_string()),
            receiver_attempt: Some(1),
            receiver_role: None,
            message_type: "request".to_string(),
            channel: None,
            payload_preview: "task hello".to_string(),
            payload_text: "task hello".to_string(),
            status: "queued".to_string(),
        })
        .expect("record task message");
    storage
        .record_ipc_message(&NewIpcMessage {
            message_id: "msg-role".to_string(),
            orchestration_id: Some(9),
            sender_pid: Some(3),
            sender_task_id: Some("planner".to_string()),
            sender_attempt: Some(1),
            receiver_pid: None,
            receiver_task_id: None,
            receiver_attempt: None,
            receiver_role: Some("reviewer".to_string()),
            message_type: "handoff".to_string(),
            channel: None,
            payload_preview: "role hello".to_string(),
            payload_text: "role hello".to_string(),
            status: "queued".to_string(),
        })
        .expect("record role message");
    storage
        .record_ipc_message(&NewIpcMessage {
            message_id: "msg-delivered".to_string(),
            orchestration_id: Some(9),
            sender_pid: Some(4),
            sender_task_id: Some("planner".to_string()),
            sender_attempt: Some(1),
            receiver_pid: None,
            receiver_task_id: Some("draft".to_string()),
            receiver_attempt: Some(1),
            receiver_role: None,
            message_type: "response".to_string(),
            channel: None,
            payload_preview: "already delivered".to_string(),
            payload_text: "already delivered".to_string(),
            status: "delivered".to_string(),
        })
        .expect("record delivered message");
    storage
        .record_ipc_message(&NewIpcMessage {
            message_id: "msg-channel".to_string(),
            orchestration_id: Some(9),
            sender_pid: Some(5),
            sender_task_id: Some("planner".to_string()),
            sender_attempt: Some(1),
            receiver_pid: None,
            receiver_task_id: None,
            receiver_attempt: None,
            receiver_role: None,
            message_type: "event".to_string(),
            channel: Some("updates".to_string()),
            payload_preview: "channel update".to_string(),
            payload_text: "channel update".to_string(),
            status: "queued".to_string(),
        })
        .expect("record channel message");

    let selector = IpcMailboxSelector {
        orchestration_id: Some(9),
        receiver_pid: Some(42),
        receiver_task_id: Some("draft".to_string()),
        receiver_role: Some("reviewer".to_string()),
        channel: None,
    };
    let queued_only = storage
        .load_ipc_mailbox_messages(&selector, false, 10)
        .expect("load queued mailbox");
    let mut queued_ids = queued_only
        .iter()
        .map(|message| message.message_id.as_str())
        .collect::<Vec<_>>();
    queued_ids.sort_unstable();
    assert_eq!(queued_ids, vec!["msg-pid", "msg-role", "msg-task"]);

    let with_delivered = storage
        .load_ipc_mailbox_messages(&selector, true, 10)
        .expect("load delivered mailbox");
    let mut delivered_ids = with_delivered
        .iter()
        .map(|message| message.message_id.as_str())
        .collect::<Vec<_>>();
    delivered_ids.sort_unstable();
    assert_eq!(
        delivered_ids,
        vec!["msg-delivered", "msg-pid", "msg-role", "msg-task"]
    );

    let channel_selector = IpcMailboxSelector {
        orchestration_id: Some(9),
        receiver_pid: None,
        receiver_task_id: None,
        receiver_role: None,
        channel: Some("updates".to_string()),
    };
    let channel_messages = storage
        .load_ipc_mailbox_messages(&channel_selector, false, 10)
        .expect("load channel mailbox");
    assert_eq!(channel_messages.len(), 1);
    assert_eq!(channel_messages[0].message_id, "msg-channel");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn ipc_status_updates_record_failed_and_consumed_timestamps() {
    let dir = make_temp_dir("agenticos_ipc_status");
    let db_path = dir.join("agenticos.db");
    let mut storage = StorageService::open(&db_path).expect("open storage");

    storage
        .record_ipc_message(&NewIpcMessage {
            message_id: "msg-1".to_string(),
            orchestration_id: Some(12),
            sender_pid: Some(7),
            sender_task_id: Some("worker".to_string()),
            sender_attempt: Some(1),
            receiver_pid: Some(8),
            receiver_task_id: None,
            receiver_attempt: None,
            receiver_role: None,
            message_type: "notification".to_string(),
            channel: None,
            payload_preview: "hello".to_string(),
            payload_text: "hello".to_string(),
            status: "queued".to_string(),
        })
        .expect("record message");

    storage
        .update_ipc_message_status("msg-1", "failed", None, None, Some(2222))
        .expect("mark failed");
    storage
        .update_ipc_message_status("msg-1", "consumed", Some(1111), Some(3333), None)
        .expect("mark consumed");

    let messages = storage
        .load_ipc_messages_by_ids(&["msg-1".to_string()])
        .expect("load by id");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].status, "consumed");
    assert_eq!(messages[0].delivered_at_ms, Some(1111));
    assert_eq!(messages[0].consumed_at_ms, Some(3333));
    assert_eq!(messages[0].failed_at_ms, Some(2222));

    let _ = fs::remove_dir_all(dir);
}

fn make_temp_dir(prefix: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), timestamp));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
