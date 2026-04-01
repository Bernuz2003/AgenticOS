use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::storage::StorageService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelinePersistenceObservation {
    pub turn_count: i64,
    pub message_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyImportObservation {
    pub imported_sessions: usize,
    pub imported_turns: usize,
    pub imported_messages: usize,
}

pub fn persist_single_turn_timeline() -> Result<TimelinePersistenceObservation, String> {
    let dir = make_temp_dir("agenticos_timeline_storage");
    let db_path = dir.join("agenticos.db");
    let mut storage = StorageService::open(&db_path).map_err(|err| err.to_string())?;
    let boot = storage
        .record_kernel_boot("0.5.0-test")
        .map_err(|err| err.to_string())?;
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
        .map_err(|err| err.to_string())?;
    storage
        .bind_session_to_pid("sess-1", "rt-test", boot.boot_id, 9, 2_000)
        .map_err(|err| err.to_string())?;

    let turn_id = storage
        .start_session_turn("sess-1", 9, "general", "exec", "hello", "prompt")
        .map_err(|err| err.to_string())?;
    storage
        .append_assistant_message(turn_id, "world")
        .map_err(|err| err.to_string())?;
    storage
        .finish_turn(turn_id, "completed", "turn_completed", None)
        .map_err(|err| err.to_string())?;

    let turn_count: i64 = storage
        .connection
        .query_row("SELECT COUNT(*) FROM session_turns", [], |row| row.get(0))
        .map_err(|err| err.to_string())?;
    let message_count: i64 = storage
        .connection
        .query_row("SELECT COUNT(*) FROM session_messages", [], |row| {
            row.get(0)
        })
        .map_err(|err| err.to_string())?;

    let _ = fs::remove_dir_all(dir);

    Ok(TimelinePersistenceObservation {
        turn_count,
        message_count,
    })
}

pub fn import_legacy_timeline_once(
) -> Result<(LegacyImportObservation, LegacyImportObservation), String> {
    let dir = make_temp_dir("agenticos_legacy_import");
    let db_path = dir.join("agenticos.db");
    let timeline_dir = dir.join("timeline_sessions");
    fs::create_dir_all(&timeline_dir).map_err(|err| err.to_string())?;
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
    .map_err(|err| err.to_string())?;

    let mut storage = StorageService::open(&db_path).map_err(|err| err.to_string())?;
    storage
        .record_kernel_boot("0.5.0-test")
        .map_err(|err| err.to_string())?;

    let first = storage
        .import_legacy_timelines_once(&timeline_dir)
        .map_err(|err| err.to_string())?;
    let second = storage
        .import_legacy_timelines_once(&timeline_dir)
        .map_err(|err| err.to_string())?;

    let _ = fs::remove_dir_all(dir);

    Ok((
        LegacyImportObservation {
            imported_sessions: first.imported_sessions,
            imported_turns: first.imported_turns,
            imported_messages: first.imported_messages,
        },
        LegacyImportObservation {
            imported_sessions: second.imported_sessions,
            imported_turns: second.imported_turns,
            imported_messages: second.imported_messages,
        },
    ))
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
