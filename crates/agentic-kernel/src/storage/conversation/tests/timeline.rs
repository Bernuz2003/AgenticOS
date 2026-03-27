use super::{LegacyTimelineImportReport, StorageService};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn session_turns_persist_user_input_chunks_and_finish_markers() {
    let dir = make_temp_dir("agenticos_timeline_storage");
    let db_path = dir.join("agenticos.db");
    let mut storage = StorageService::open(&db_path).expect("open storage");
    let boot = storage
        .record_kernel_boot("0.5.0-test")
        .expect("record boot");
    storage
        .insert_session(
            "sess-1",
            "Session one",
            "idle",
            Some("rt-test"),
            None,
            1_000,
            1_000,
        )
        .expect("insert session");
    storage
        .bind_session_to_pid("sess-1", "rt-test", boot.boot_id, 9, 2_000)
        .expect("bind session");

    let turn_id = storage
        .start_session_turn("sess-1", 9, "general", "exec", "hello", "prompt")
        .expect("start turn");
    storage
        .append_assistant_message(turn_id, "world")
        .expect("append assistant");
    storage
        .finish_turn(turn_id, "completed", "turn_completed", None)
        .expect("finish turn");

    let turn_count: i64 = storage
        .connection
        .query_row("SELECT COUNT(*) FROM session_turns", [], |row| row.get(0))
        .expect("count turns");
    let message_count: i64 = storage
        .connection
        .query_row("SELECT COUNT(*) FROM session_messages", [], |row| {
            row.get(0)
        })
        .expect("count messages");

    assert_eq!(turn_count, 1);
    assert_eq!(message_count, 2);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn legacy_timeline_import_is_idempotent() {
    let dir = make_temp_dir("agenticos_legacy_import");
    let db_path = dir.join("agenticos.db");
    let timeline_dir = dir.join("timeline_sessions");
    fs::create_dir_all(&timeline_dir).expect("create timeline dir");
    fs::write(
        timeline_dir.join("pid-7.json"),
        serde_json::json!({
            "session_id": "pid-7",
            "pid": 7,
            "workload": "general",
            "turns": [
                {
                    "prompt": "legacy prompt",
                    "assistant_stream": "legacy answer",
                    "running": false
                }
            ],
            "error": null,
            "system_events": [["legacy note", "complete"]]
        })
        .to_string(),
    )
    .expect("write legacy timeline");

    let mut storage = StorageService::open(&db_path).expect("open storage");
    storage
        .record_kernel_boot("0.5.0-test")
        .expect("record boot");

    let first = storage
        .import_legacy_timelines_once(&timeline_dir)
        .expect("import legacy timelines");
    let second = storage
        .import_legacy_timelines_once(&timeline_dir)
        .expect("skip already imported timelines");

    assert_eq!(
        first,
        LegacyTimelineImportReport {
            imported_sessions: 1,
            imported_turns: 1,
            imported_messages: 3,
        }
    );
    assert_eq!(second, LegacyTimelineImportReport::default());

    let _ = fs::remove_dir_all(dir);
}

fn make_temp_dir(prefix: &str) -> std::path::PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), timestamp));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
