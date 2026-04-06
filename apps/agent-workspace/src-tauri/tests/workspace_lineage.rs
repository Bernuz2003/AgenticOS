use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_workspace_lib::test_support::composer::{
    compose_workspace_snapshot_for_session, offline_bridge,
};
use agent_workspace_lib::test_support::history::load_lobby_sessions;
use agent_workspace_lib::test_support::live_timeline::TimelineStore;
use rusqlite::{params, Connection};
use serde_json::json;

#[test]
fn source_and_replay_sessions_project_a_single_lineage() {
    let root = make_temp_root("agenticos-workspace-lineage");
    let db_path = root.join("workspace").join("agenticos.db");
    fs::create_dir_all(db_path.parent().expect("db parent")).expect("create workspace dir");
    let connection = Connection::open(&db_path).expect("open db");
    seed_lineage_fixture(&connection);

    let bridge = offline_bridge(root.clone());
    let timeline_store = Arc::new(Mutex::new(TimelineStore::default()));

    let source_snapshot = compose_workspace_snapshot_for_session(
        &root,
        &bridge,
        &timeline_store,
        "sess-source",
        None,
    )
    .expect("compose source snapshot");
    let source_lineage = source_snapshot.lineage.expect("source lineage");
    let source_lineage_json = serde_json::to_value(&source_lineage).expect("serialize lineage");
    assert_eq!(source_lineage.anchor_session_id, "sess-source");
    assert_eq!(source_lineage.selected_session_id, "sess-source");
    assert_eq!(source_lineage_json["selected_kind"], json!("base"));
    assert_eq!(source_lineage.branches.len(), 2);
    assert_eq!(source_lineage.branches[0].session_id, "sess-source");
    assert_eq!(source_lineage_json["branches"][0]["kind"], json!("base"));
    assert_eq!(source_lineage.branches[1].session_id, "sess-replay-1");
    assert_eq!(source_lineage_json["branches"][1]["kind"], json!("replay"));
    assert_eq!(
        source_lineage.branches[1].source_dump_id.as_deref(),
        Some("dump-001")
    );

    let replay_snapshot = compose_workspace_snapshot_for_session(
        &root,
        &bridge,
        &timeline_store,
        "sess-replay-1",
        None,
    )
    .expect("compose replay snapshot");
    let replay_lineage = replay_snapshot.lineage.expect("replay lineage");
    let replay_lineage_json = serde_json::to_value(&replay_lineage).expect("serialize lineage");
    assert_eq!(replay_lineage.anchor_session_id, "sess-source");
    assert_eq!(replay_lineage.selected_session_id, "sess-replay-1");
    assert_eq!(replay_lineage_json["selected_kind"], json!("replay"));
    assert_eq!(replay_lineage.branches.len(), 2);
    assert!(replay_lineage.branches[1].selected);

    fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lobby_projection_hides_replay_branch_sessions() {
    let root = make_temp_root("agenticos-lobby-lineage");
    let db_path = root.join("workspace").join("agenticos.db");
    fs::create_dir_all(db_path.parent().expect("db parent")).expect("create workspace dir");
    let connection = Connection::open(&db_path).expect("open db");
    seed_lineage_fixture(&connection);

    let sessions = load_lobby_sessions(&root).expect("load lobby sessions");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "sess-source");

    fs::remove_dir_all(root).expect("remove temp root");
}

fn seed_lineage_fixture(connection: &Connection) {
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
        .expect("create lineage schema");

    connection
        .execute(
            "INSERT INTO sessions(session_id, title, status, active_pid, created_at_ms, updated_at_ms) VALUES (?1, ?2, 'idle', NULL, ?3, ?4)",
            params!["sess-source", "Source Session", 1_000_i64, 1_200_i64],
        )
        .expect("insert source session");
    connection
        .execute(
            "INSERT INTO process_runs(session_id, boot_id, pid, state, started_at_ms, ended_at_ms) VALUES (?1, 1, ?2, 'completed', ?3, ?4)",
            params!["sess-source", 410_u64, 1_000_i64, 1_100_i64],
        )
        .expect("insert source process run");
    connection
        .execute(
            "INSERT INTO session_turns(session_id, pid, turn_index, workload, source, status, started_at_ms, updated_at_ms, completed_at_ms, finish_reason) VALUES (?1, ?2, 1, 'general', 'interactive', 'completed', 1000, 1200, 1200, 'turn_completed')",
            params!["sess-source", 410_u64],
        )
        .expect("insert source turn");
    let source_turn_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO session_messages(session_id, turn_id, pid, ordinal, role, kind, content, created_at_ms) VALUES (?1, ?2, ?3, 1, 'user', 'prompt', 'source prompt', 1000)",
            params!["sess-source", source_turn_id, 410_u64],
        )
        .expect("insert source prompt");

    connection
        .execute(
            "INSERT INTO sessions(session_id, title, status, active_pid, created_at_ms, updated_at_ms) VALUES (?1, ?2, 'idle', NULL, ?3, ?4)",
            params!["sess-replay-1", "[Replay] Branch 1", 1_300_i64, 1_350_i64],
        )
        .expect("insert replay session");
    connection
        .execute(
            "INSERT INTO process_runs(session_id, boot_id, pid, state, started_at_ms, ended_at_ms) VALUES (?1, 1, ?2, 'completed', ?3, ?4)",
            params!["sess-replay-1", 510_u64, 1_300_i64, 1_340_i64],
        )
        .expect("insert replay process run");
    connection
        .execute(
            "INSERT INTO session_turns(session_id, pid, turn_index, workload, source, status, started_at_ms, updated_at_ms, completed_at_ms, finish_reason) VALUES (?1, ?2, 1, 'general', 'core_dump_replay_import', 'completed', 1300, 1350, 1350, 'core_dump_replay_import')",
            params!["sess-replay-1", 510_u64],
        )
        .expect("insert replay turn");
    let replay_turn_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO session_messages(session_id, turn_id, pid, ordinal, role, kind, content, created_at_ms) VALUES (?1, ?2, ?3, 1, 'user', 'prompt', 'replay prompt', 1300)",
            params!["sess-replay-1", replay_turn_id, 510_u64],
        )
        .expect("insert replay prompt");

    connection
        .execute(
            "INSERT INTO replay_branch_index(session_id, created_at_ms, pid, source_dump_id, source_session_id, source_pid, source_fidelity, replay_mode, tool_mode, initial_state, patched_context_segments, patched_episodic_segments, stubbed_invocations, overridden_invocations, baseline_json) VALUES (?1, 1300, ?2, 'dump-001', 'sess-source', 410, 'full_context_snapshot', 'isolated_counterfactual_branch', 'stubbed_recorded_tools', 'Ready', 0, 0, 0, 0, ?3)",
            params![
                "sess-replay-1",
                510_u64,
                r#"{"context_segments":[],"episodic_segments":[],"replay_messages":[],"tool_invocations":[]}"#
            ],
        )
        .expect("insert replay branch metadata");
}

fn make_temp_root(prefix: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{unique}"))
}
