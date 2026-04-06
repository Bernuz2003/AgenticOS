use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_workspace_lib::test_support::composer::{
    compose_timeline_snapshot_for_session, compose_workspace_snapshot_for_session, offline_bridge,
    register_live_user_input,
};
use agent_workspace_lib::test_support::live_timeline::TimelineStore;
use rusqlite::{params, Connection};

#[test]
fn workspace_composer_falls_back_to_persisted_snapshot_when_live_bridge_is_unavailable() {
    let root = make_temp_root("agenticos-composer-workspace");
    seed_persisted_session(
        &root,
        SessionSeed::single_completed("sess-compose", 44, "hello composer", "persisted answer"),
    )
    .expect("seed session");
    let bridge = offline_bridge(root.clone());
    let timeline_store = Arc::new(Mutex::new(TimelineStore::default()));

    let snapshot = compose_workspace_snapshot_for_session(
        &root,
        &bridge,
        &timeline_store,
        "sess-compose",
        None,
    )
    .expect("compose workspace snapshot");

    assert_eq!(snapshot.session_id, "sess-compose");
    assert_eq!(snapshot.pid, 44);
    assert_eq!(snapshot.state, "Finished");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn timeline_composer_falls_back_to_persisted_history_when_live_bridge_is_unavailable() {
    let root = make_temp_root("agenticos-composer-timeline");
    seed_persisted_session(
        &root,
        SessionSeed::single_completed("sess-compose", 44, "hello composer", "persisted answer"),
    )
    .expect("seed session");
    let bridge = offline_bridge(root.clone());
    let timeline_store = Arc::new(Mutex::new(TimelineStore::default()));

    let timeline = compose_timeline_snapshot_for_session(
        &root,
        &bridge,
        &timeline_store,
        "sess-compose",
        None,
    )
    .expect("compose timeline snapshot");

    assert_eq!(timeline.session_id, "sess-compose");
    assert_eq!(timeline.pid, 44);
    assert_eq!(timeline.source, "sqlite_history");
    assert_eq!(timeline.items.len(), 2);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_composer_ignores_foreign_pid_hints_when_loading_by_session() {
    let root = make_temp_root("agenticos-composer-foreign-workspace");
    seed_persisted_session(
        &root,
        SessionSeed::single_completed("sess-one", 11, "hello one", "answer one"),
    )
    .expect("seed first session");
    seed_persisted_session(
        &root,
        SessionSeed::single_completed("sess-two", 22, "hello two", "answer two"),
    )
    .expect("seed second session");
    let bridge = offline_bridge(root.clone());
    let timeline_store = Arc::new(Mutex::new(TimelineStore::default()));

    let snapshot = compose_workspace_snapshot_for_session(
        &root,
        &bridge,
        &timeline_store,
        "sess-two",
        Some(11),
    )
    .expect("compose workspace snapshot");

    assert_eq!(snapshot.session_id, "sess-two");
    assert_eq!(snapshot.pid, 22);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn timeline_composer_ignores_foreign_pid_hints_when_loading_by_session() {
    let root = make_temp_root("agenticos-composer-foreign-timeline");
    seed_persisted_session(
        &root,
        SessionSeed::single_completed("sess-one", 11, "hello one", "answer one"),
    )
    .expect("seed first session");
    seed_persisted_session(
        &root,
        SessionSeed::single_completed("sess-two", 22, "hello two", "answer two"),
    )
    .expect("seed second session");
    let bridge = offline_bridge(root.clone());
    let timeline_store = Arc::new(Mutex::new(TimelineStore::default()));

    let timeline = compose_timeline_snapshot_for_session(
        &root,
        &bridge,
        &timeline_store,
        "sess-two",
        Some(11),
    )
    .expect("compose timeline snapshot");

    assert_eq!(timeline.session_id, "sess-two");
    assert_eq!(timeline.pid, 22);
    assert_eq!(timeline.items[0].text, "hello two");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn register_live_user_input_appends_new_turn_after_seeding_existing_history() {
    let root = make_temp_root("agenticos-composer-resume-seed");
    seed_persisted_session(
        &root,
        SessionSeed::single_completed("sess-compose", 44, "hello composer", "persisted answer"),
    )
    .expect("seed session");
    let timeline_store = Arc::new(Mutex::new(TimelineStore::default()));

    register_live_user_input(
        &root,
        &timeline_store,
        "sess-compose",
        84,
        Some("general".to_string()),
        "new input",
    )
    .expect("register live user input");

    let timeline = timeline_store
        .lock()
        .expect("timeline store")
        .snapshot(84)
        .expect("seeded live timeline");
    assert_eq!(timeline.session_id, "sess-compose");
    assert_eq!(timeline.items.len(), 4);
    assert_eq!(timeline.items[0].text, "hello composer");
    assert_eq!(timeline.items[1].text, "persisted answer");
    assert_eq!(timeline.items[2].text, "new input");
    assert!(timeline.running);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn register_live_user_input_does_not_duplicate_prompt_already_present_in_seeded_history() {
    let root = make_temp_root("agenticos-composer-no-dup");
    seed_persisted_session(
        &root,
        SessionSeed::with_running_turn(
            "sess-compose",
            44,
            "hello composer",
            "persisted answer",
            "new input",
        ),
    )
    .expect("seed session");
    let timeline_store = Arc::new(Mutex::new(TimelineStore::default()));

    register_live_user_input(
        &root,
        &timeline_store,
        "sess-compose",
        84,
        Some("general".to_string()),
        "new input",
    )
    .expect("register live user input");

    let timeline = timeline_store
        .lock()
        .expect("timeline store")
        .snapshot(84)
        .expect("seeded live timeline");
    let new_input_count = timeline
        .items
        .iter()
        .filter(|item| item.text == "new input")
        .count();
    assert_eq!(new_input_count, 1);
    assert_eq!(timeline.items.len(), 4);
    assert!(timeline.running);

    let _ = fs::remove_dir_all(root);
}

#[derive(Clone)]
struct SessionSeed<'a> {
    session_id: &'a str,
    pid: u64,
    turns: Vec<SessionTurnSeed<'a>>,
    session_status: &'a str,
    run_state: &'a str,
}

#[derive(Clone)]
struct SessionTurnSeed<'a> {
    status: &'a str,
    finish_reason: Option<&'a str>,
    prompt: &'a str,
    assistant: Option<&'a str>,
}

impl<'a> SessionSeed<'a> {
    fn single_completed(
        session_id: &'a str,
        pid: u64,
        prompt: &'a str,
        assistant: &'a str,
    ) -> Self {
        Self {
            session_id,
            pid,
            turns: vec![SessionTurnSeed {
                status: "completed",
                finish_reason: Some("turn_completed"),
                prompt,
                assistant: Some(assistant),
            }],
            session_status: "completed",
            run_state: "completed",
        }
    }

    fn with_running_turn(
        session_id: &'a str,
        pid: u64,
        initial_prompt: &'a str,
        initial_assistant: &'a str,
        running_prompt: &'a str,
    ) -> Self {
        Self {
            session_id,
            pid,
            turns: vec![
                SessionTurnSeed {
                    status: "completed",
                    finish_reason: Some("turn_completed"),
                    prompt: initial_prompt,
                    assistant: Some(initial_assistant),
                },
                SessionTurnSeed {
                    status: "running",
                    finish_reason: None,
                    prompt: running_prompt,
                    assistant: None,
                },
            ],
            session_status: "running",
            run_state: "running",
        }
    }
}

fn seed_persisted_session(root: &Path, seed: SessionSeed<'_>) -> Result<(), String> {
    let db_path = root.join("workspace").join("agenticos.db");
    fs::create_dir_all(db_path.parent().expect("db parent")).map_err(|err| err.to_string())?;
    let connection = Connection::open(&db_path).map_err(|err| err.to_string())?;
    connection
        .execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                status TEXT NOT NULL,
                runtime_id TEXT NULL,
                active_pid INTEGER NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS process_runs (
                run_id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                boot_id INTEGER NOT NULL,
                pid INTEGER NOT NULL,
                state TEXT NOT NULL,
                started_at_ms INTEGER NOT NULL,
                ended_at_ms INTEGER NULL
            );
            CREATE TABLE IF NOT EXISTS session_turns (
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
            CREATE TABLE IF NOT EXISTS session_messages (
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
            "#,
        )
        .map_err(|err| err.to_string())?;
    let now = timestamp_ms();
    let active_pid = if seed.session_status == "running" {
        Some(seed.pid as i64)
    } else {
        None
    };
    connection
        .execute(
            "INSERT INTO sessions VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?5)",
            params![
                seed.session_id,
                "Persisted session",
                seed.session_status,
                active_pid,
                now
            ],
        )
        .map_err(|err| err.to_string())?;
    connection
        .execute(
            "INSERT INTO process_runs (session_id, boot_id, pid, state, started_at_ms, ended_at_ms)
             VALUES (?1, 1, ?2, ?3, ?4, ?4)",
            params![seed.session_id, seed.pid, seed.run_state, now],
        )
        .map_err(|err| err.to_string())?;

    for (index, turn) in seed.turns.iter().enumerate() {
        connection
            .execute(
                "INSERT INTO session_turns VALUES (NULL, ?1, ?2, ?3, 'general', 'legacy', ?4, ?5, ?5, ?6, ?7)",
                params![
                    seed.session_id,
                    seed.pid,
                    (index + 1) as i64,
                    turn.status,
                    now,
                    if turn.status == "running" {
                        None::<i64>
                    } else {
                        Some(now)
                    },
                    turn.finish_reason,
                ],
            )
            .map_err(|err| err.to_string())?;
        let turn_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO session_messages VALUES (NULL, ?1, ?2, ?3, 1, 'user', 'prompt', ?4, ?5)",
                params![seed.session_id, turn_id, seed.pid, turn.prompt, now],
            )
            .map_err(|err| err.to_string())?;
        if let Some(assistant) = turn.assistant {
            connection
                .execute(
                    "INSERT INTO session_messages VALUES (NULL, ?1, ?2, ?3, 2, 'assistant', 'message', ?4, ?5)",
                    params![seed.session_id, turn_id, seed.pid, assistant, now],
                )
                .map_err(|err| err.to_string())?;
        }
    }

    Ok(())
}

fn make_temp_root(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nonce}"))
}

fn timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("epoch")
        .as_millis() as i64
}
