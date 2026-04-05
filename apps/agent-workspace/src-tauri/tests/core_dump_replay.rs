use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_workspace_lib::test_support::composer::{
    compose_workspace_snapshot_for_session, offline_bridge,
};
use agent_workspace_lib::test_support::live_timeline::TimelineStore;
use rusqlite::{params, Connection};

#[test]
fn persisted_workspace_snapshot_surfaces_replay_branch_diff() {
    let root = make_temp_root("agenticos-tauri-replay");
    let db_path = root.join("workspace").join("agenticos.db");
    fs::create_dir_all(db_path.parent().expect("db parent")).expect("create workspace dir");
    let connection = Connection::open(&db_path).expect("open db");
    connection
        .execute_batch(
            r#"
            CREATE TABLE sessions (
                session_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                active_pid INTEGER NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            CREATE TABLE process_runs (
                run_id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                boot_id INTEGER NOT NULL,
                pid INTEGER NOT NULL,
                state TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                ended_at_ms INTEGER NULL
            );
            CREATE TABLE session_turns (
                turn_id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                pid INTEGER NOT NULL,
                turn_index INTEGER NOT NULL,
                workload TEXT NOT NULL,
                source TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                completed_at_ms INTEGER NULL,
                finish_reason TEXT NULL
            );
            CREATE TABLE session_messages (
                message_id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                turn_id INTEGER NOT NULL,
                pid INTEGER NOT NULL,
                ordinal INTEGER NOT NULL,
                role TEXT NOT NULL,
                kind TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL
            );
            CREATE TABLE tool_invocation_history (
                invocation_id INTEGER PRIMARY KEY AUTOINCREMENT,
                tool_call_id TEXT NOT NULL UNIQUE,
                recorded_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                session_id TEXT NULL,
                pid INTEGER NULL,
                runtime_id TEXT NULL,
                tool_name TEXT NOT NULL,
                caller TEXT NOT NULL,
                transport TEXT NOT NULL,
                status TEXT NOT NULL,
                command_text TEXT NOT NULL,
                input_json TEXT NOT NULL,
                output_json TEXT NULL,
                output_text TEXT NULL,
                warnings_json TEXT NULL,
                error_kind TEXT NULL,
                error_text TEXT NULL,
                effect_json TEXT NULL,
                duration_ms INTEGER NULL,
                kill INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE replay_branch_index (
                session_id TEXT PRIMARY KEY,
                created_at_ms INTEGER NOT NULL,
                pid INTEGER NOT NULL,
                source_dump_id TEXT NOT NULL,
                source_session_id TEXT NULL,
                source_pid INTEGER NULL,
                source_fidelity TEXT NOT NULL,
                replay_mode TEXT NOT NULL,
                tool_mode TEXT NOT NULL,
                initial_state TEXT NOT NULL,
                patched_context_segments INTEGER NOT NULL DEFAULT 0,
                patched_episodic_segments INTEGER NOT NULL DEFAULT 0,
                stubbed_invocations INTEGER NOT NULL DEFAULT 0,
                overridden_invocations INTEGER NOT NULL DEFAULT 0,
                baseline_json TEXT NOT NULL
            );
            "#,
        )
        .expect("create replay snapshot fixture");

    connection
        .execute(
            "INSERT INTO sessions(session_id, title, status, active_pid, created_at_ms, updated_at_ms) VALUES (?1, ?2, ?3, NULL, ?4, ?4)",
            params!["sess-replay", "[Replay] branch", "idle", 1_000_i64],
        )
        .expect("insert session");
    connection
        .execute(
            "INSERT INTO process_runs(session_id, boot_id, pid, state, started_at_ms, ended_at_ms) VALUES (?1, 1, ?2, 'idle', ?3, ?4)",
            params!["sess-replay", 401_u64, 1_000_i64, 1_100_i64],
        )
        .expect("insert process run");
    connection
        .execute(
            "INSERT INTO session_turns(session_id, pid, turn_index, workload, source, status, started_at_ms, updated_at_ms, completed_at_ms, finish_reason) VALUES (?1, ?2, 1, 'fast', 'core_dump_replay_import', 'completed', 1000, 1000, 1000, 'core_dump_replay_import')",
            params!["sess-replay", 401_u64],
        )
        .expect("insert turn 1");
    let turn_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO session_messages(session_id, turn_id, pid, ordinal, role, kind, content, created_at_ms) VALUES (?1, ?2, ?3, 1, 'user', 'prompt', 'Original prompt', 1000)",
            params!["sess-replay", turn_id, 401_u64],
        )
        .expect("insert user message");
    connection
        .execute(
            "INSERT INTO session_messages(session_id, turn_id, pid, ordinal, role, kind, content, created_at_ms) VALUES (?1, ?2, ?3, 2, 'assistant', 'message', 'Original answer', 1000)",
            params!["sess-replay", turn_id, 401_u64],
        )
        .expect("insert assistant message");
    connection
        .execute(
            "INSERT INTO session_turns(session_id, pid, turn_index, workload, source, status, started_at_ms, updated_at_ms, completed_at_ms, finish_reason) VALUES (?1, ?2, 2, 'fast', 'interactive', 'completed', 1010, 1010, 1010, 'model_stop')",
            params!["sess-replay", 401_u64],
        )
        .expect("insert turn 2");
    let turn_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO session_messages(session_id, turn_id, pid, ordinal, role, kind, content, created_at_ms) VALUES (?1, ?2, ?3, 1, 'user', 'prompt', 'Counterfactual question', 1010)",
            params!["sess-replay", turn_id, 401_u64],
        )
        .expect("insert counterfactual prompt");
    connection
        .execute(
            "INSERT INTO session_messages(session_id, turn_id, pid, ordinal, role, kind, content, created_at_ms) VALUES (?1, ?2, ?3, 2, 'assistant', 'message', 'Counterfactual answer', 1010)",
            params!["sess-replay", turn_id, 401_u64],
        )
        .expect("insert counterfactual answer");

    connection
        .execute(
            "INSERT INTO tool_invocation_history(tool_call_id, recorded_at_ms, updated_at_ms, session_id, pid, tool_name, caller, transport, status, command_text, input_json, output_text, warnings_json, error_kind, kill) VALUES (?1, 1020, 1021, ?2, ?3, 'calc', 'agent_text', 'text', 'completed', ?4, '{}', '5', ?5, NULL, 0)",
            params![
                "tool-call-replay-1",
                "sess-replay",
                401_u64,
                r#"TOOL:calc {"expression":"2+2"}"#,
                r#"["replay_stub_source_call_id=tool-call-source-1"]"#,
            ],
        )
        .expect("insert changed tool invocation");
    connection
        .execute(
            "INSERT INTO tool_invocation_history(tool_call_id, recorded_at_ms, updated_at_ms, session_id, pid, tool_name, caller, transport, status, command_text, input_json, output_text, warnings_json, error_kind, kill) VALUES (?1, 1030, 1031, ?2, ?3, 'search', 'agent_text', 'text', 'completed', ?4, '{}', 'new branch result', NULL, NULL, 0)",
            params![
                "tool-call-replay-2",
                "sess-replay",
                401_u64,
                r#"TOOL:search {"q":"branch only"}"#,
            ],
        )
        .expect("insert branch-only tool invocation");

    let baseline_json = serde_json::json!({
        "context_segments": [
            { "kind": "user_turn", "text": "Original prompt\n" },
            { "kind": "tool_output", "text": "\nOutput:\n4\n" }
        ],
        "episodic_segments": [
            { "kind": "summary", "text": "baseline episodic memory" }
        ],
        "replay_messages": [
            { "role": "user", "kind": "prompt", "content": "Original prompt" },
            { "role": "assistant", "kind": "message", "content": "Original answer" }
        ],
        "tool_invocations": [
            {
                "tool_call_id": "tool-call-source-1",
                "tool_name": "calc",
                "command_text": r#"TOOL:calc {"expression":"2+2"}"#,
                "status": "completed",
                "output_text": "4",
                "error_kind": null,
                "kill": false
            }
        ]
    })
    .to_string();
    connection
        .execute(
            "INSERT INTO replay_branch_index(session_id, created_at_ms, pid, source_dump_id, source_session_id, source_pid, source_fidelity, replay_mode, tool_mode, initial_state, patched_context_segments, patched_episodic_segments, stubbed_invocations, overridden_invocations, baseline_json) VALUES (?1, 1005, ?2, 'dump-001', 'sess-source', 77, 'full_context_snapshot', 'isolated_counterfactual_branch', 'stubbed_recorded_tools', 'Ready', 2, 1, 1, 1, ?3)",
            params!["sess-replay", 401_u64, baseline_json],
        )
        .expect("insert replay branch metadata");

    let bridge = offline_bridge(root.clone());
    let timeline_store = Arc::new(Mutex::new(TimelineStore::default()));
    let snapshot = compose_workspace_snapshot_for_session(
        &root,
        &bridge,
        &timeline_store,
        "sess-replay",
        None,
    )
    .expect("compose workspace snapshot");

    let replay = snapshot.replay.expect("replay snapshot");
    assert_eq!(replay.source_dump_id, "dump-001");
    assert_eq!(replay.baseline.source_replay_messages, 2);
    assert_eq!(replay.diff.current_replay_messages, 4);
    assert_eq!(replay.diff.replay_messages_delta, 2);
    assert_eq!(replay.diff.branch_only_messages, 2);
    assert_eq!(replay.diff.current_tool_invocations, 2);
    assert_eq!(replay.diff.changed_tool_outputs, 1);
    assert_eq!(replay.diff.branch_only_tool_calls, 1);
    assert_eq!(
        replay.diff.latest_branch_message.as_deref(),
        Some("Counterfactual answer")
    );
    assert_eq!(replay.diff.invocation_diffs.len(), 2);
    assert_eq!(
        replay.diff.invocation_diffs[0]
            .source_tool_call_id
            .as_deref(),
        Some("tool-call-source-1")
    );
    assert_eq!(
        replay.diff.invocation_diffs[0]
            .replay_output_text
            .as_deref(),
        Some("5")
    );
    assert!(replay.diff.invocation_diffs[1].branch_only);

    fs::remove_dir_all(root).expect("remove temp root");
}

fn make_temp_root(prefix: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{unique}"))
}
