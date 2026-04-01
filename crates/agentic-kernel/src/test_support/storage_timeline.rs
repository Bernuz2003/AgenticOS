use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use agentic_control_models::AssistantSegmentKind;
use rusqlite::params;

use crate::storage::StorageService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelinePersistenceObservation {
    pub turn_count: i64,
    pub message_count: i64,
    pub thinking_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyImportObservation {
    pub imported_sessions: usize,
    pub imported_turns: usize,
    pub imported_messages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayMessageObservation {
    pub role: String,
    pub kind: String,
    pub content: String,
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
        .append_assistant_segment(turn_id, AssistantSegmentKind::Thinking, "internal note")
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
    let thinking_count: i64 = storage
        .connection
        .query_row(
            "SELECT COUNT(*) FROM session_messages WHERE role = 'assistant' AND kind = 'thinking'",
            [],
            |row| row.get(0),
        )
        .map_err(|err| err.to_string())?;

    let _ = fs::remove_dir_all(dir);

    Ok(TimelinePersistenceObservation {
        turn_count,
        message_count,
        thinking_count,
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
                    "assistant_stream": "Prelude\n<think>legacy reasoning</think>\nAfter",
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

pub fn normalize_legacy_assistant_messages_on_reopen(
) -> Result<Vec<ReplayMessageObservation>, String> {
    let dir = make_temp_dir("agenticos_legacy_thinking_normalization");
    let db_path = dir.join("agenticos.db");
    let mut storage = StorageService::open(&db_path).map_err(|err| err.to_string())?;
    let boot = storage
        .record_kernel_boot("0.5.0-test")
        .map_err(|err| err.to_string())?;
    storage
        .insert_session(
            "sess-legacy-think",
            "Legacy thinking",
            "idle",
            Some("rt-test"),
            None,
            1_000,
            1_000,
        )
        .map_err(|err| err.to_string())?;
    storage
        .bind_session_to_pid("sess-legacy-think", "rt-test", boot.boot_id, 9, 2_000)
        .map_err(|err| err.to_string())?;

    let first_turn_id = storage
        .start_session_turn(
            "sess-legacy-think",
            9,
            "general",
            "exec",
            "first prompt",
            "prompt",
        )
        .map_err(|err| err.to_string())?;
    storage
        .append_assistant_message(
            first_turn_id,
            "Prelude\n<think>legacy reasoning</think>\nAfter",
        )
        .map_err(|err| err.to_string())?;
    storage
        .connection
        .execute(
            "UPDATE session_messages SET kind = 'chunk' WHERE turn_id = ?1 AND role = 'assistant'",
            params![first_turn_id],
        )
        .map_err(|err| err.to_string())?;
    storage
        .finish_turn(first_turn_id, "completed", "turn_completed", None)
        .map_err(|err| err.to_string())?;

    let second_turn_id = storage
        .start_session_turn(
            "sess-legacy-think",
            9,
            "general",
            "exec",
            "second prompt",
            "input",
        )
        .map_err(|err| err.to_string())?;
    storage
        .append_assistant_message(second_turn_id, "second answer")
        .map_err(|err| err.to_string())?;
    storage
        .finish_turn(second_turn_id, "completed", "turn_completed", None)
        .map_err(|err| err.to_string())?;
    storage
        .connection
        .execute(
            "DELETE FROM kernel_meta WHERE key = ?1",
            params!["assistant_thinking_normalization_v1_completed_at_ms"],
        )
        .map_err(|err| err.to_string())?;

    drop(storage);

    let storage = StorageService::open(&db_path).map_err(|err| err.to_string())?;
    let replay_messages = storage
        .load_replay_messages_for_session("sess-legacy-think")
        .map_err(|err| err.to_string())?;
    let observations = replay_messages
        .into_iter()
        .map(|message| ReplayMessageObservation {
            role: message.role,
            kind: message.kind,
            content: message.content,
        })
        .collect();

    let _ = fs::remove_dir_all(dir);

    Ok(observations)
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
