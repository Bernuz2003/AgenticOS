use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(crate) struct SessionIdentity {
    pub(crate) session_id: String,
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) active_pid: Option<u64>,
    pub(crate) last_pid: Option<u64>,
    pub(crate) runtime_id: Option<String>,
    pub(crate) runtime_label: Option<String>,
    pub(crate) backend_class: Option<String>,
    pub(crate) workload: String,
    pub(crate) updated_at_ms: i64,
    pub(crate) turn_count: usize,
    pub(crate) prompt_preview: String,
}

#[derive(Debug)]
pub(crate) struct StoredTurn {
    pub(crate) turn_id: i64,
    pub(crate) turn_index: i64,
    pub(crate) pid: u64,
    pub(crate) workload: String,
    pub(crate) status: String,
    pub(crate) finish_reason: Option<String>,
}

#[derive(Debug)]
pub(crate) struct StoredMessage {
    pub(crate) turn_id: i64,
    pub(crate) ordinal: i64,
    pub(crate) role: String,
    pub(crate) kind: String,
    pub(crate) content: String,
}

#[derive(Debug)]
pub(crate) struct StoredAuditRow {
    pub(crate) recorded_at_ms: i64,
    pub(crate) category: String,
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) session_id: Option<String>,
    pub(crate) pid: Option<u64>,
    pub(crate) runtime_id: Option<String>,
}

pub(crate) fn open_connection(workspace_root: &Path) -> Result<Option<Connection>, String> {
    let path = database_path(workspace_root);
    if !path.exists() {
        return Ok(None);
    }

    let connection = Connection::open(path).map_err(|err| err.to_string())?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|err| err.to_string())?;
    Ok(Some(connection))
}

fn database_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join("workspace").join("agenticos.db")
}

pub(crate) fn load_all_session_identities(
    connection: &Connection,
) -> Result<Vec<SessionIdentity>, String> {
    let runtime_metadata_enabled = runtime_metadata_available(connection)?;
    let query = if runtime_metadata_enabled {
        session_identity_select_query(None)
    } else {
        session_identity_legacy_select_query(None)
    };
    let mut statement = connection.prepare(&query).map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], map_session_identity_row)
        .map_err(|err| err.to_string())?;

    let mut sessions = Vec::new();
    for row in rows {
        sessions.push(row.map_err(|err| err.to_string())?);
    }

    Ok(sessions)
}

pub(crate) fn load_session_identity(
    connection: &Connection,
    session_id: &str,
) -> Result<Option<SessionIdentity>, String> {
    let runtime_metadata_enabled = runtime_metadata_available(connection)?;
    let query = if runtime_metadata_enabled {
        session_identity_select_query(Some("?1"))
    } else {
        session_identity_legacy_select_query(Some("?1"))
    };
    connection
        .query_row(&query, params![session_id], map_session_identity_row)
        .optional()
        .map_err(|err| err.to_string())
}

pub(crate) fn session_identity_select_query(filter_placeholder: Option<&str>) -> String {
    let filter = filter_placeholder
        .map(|placeholder| format!("WHERE s.session_id = {placeholder}"))
        .unwrap_or_default();
    format!(
        r#"
        SELECT
            s.session_id,
            s.title,
            s.status,
            s.active_pid,
            COALESCE(
                s.active_pid,
                (
                    SELECT pr.pid
                    FROM process_runs pr
                    WHERE pr.session_id = s.session_id
                    ORDER BY pr.run_id DESC
                    LIMIT 1
                )
            ) AS last_pid,
            s.runtime_id,
            COALESCE(
                CASE
                    WHEN ri.provider_id IS NOT NULL THEN ri.provider_id || ' · ' || COALESCE(ri.remote_model_id, ri.logical_model_id)
                    WHEN ri.logical_model_id != '' THEN ri.logical_model_id
                    ELSE ri.runtime_id
                END,
                s.runtime_id,
                'runtime-unbound'
            ) AS runtime_label,
            ri.backend_class,
            COALESCE(
                (
                    SELECT st.workload
                    FROM session_turns st
                    WHERE st.session_id = s.session_id
                    ORDER BY st.turn_index DESC
                    LIMIT 1
                ),
                'general'
            ) AS workload,
            s.updated_at_ms,
            COALESCE(
                (
                    SELECT COUNT(*)
                    FROM session_turns st
                    WHERE st.session_id = s.session_id
                ),
                0
            ) AS turn_count,
            COALESCE(
                (
                    SELECT sm.content
                    FROM session_messages sm
                    JOIN session_turns st ON st.turn_id = sm.turn_id
                    WHERE sm.session_id = s.session_id
                      AND sm.role = 'user'
                    ORDER BY st.turn_index DESC, sm.ordinal ASC
                    LIMIT 1
                ),
                s.title
            ) AS prompt_preview
        FROM sessions s
        LEFT JOIN runtime_instances ri ON ri.runtime_id = s.runtime_id
        {filter}
        ORDER BY s.updated_at_ms DESC
        "#
    )
}

pub(crate) fn session_identity_legacy_select_query(filter_placeholder: Option<&str>) -> String {
    let filter = filter_placeholder
        .map(|placeholder| format!("WHERE s.session_id = {placeholder}"))
        .unwrap_or_default();
    format!(
        r#"
        SELECT
            s.session_id,
            s.title,
            s.status,
            s.active_pid,
            COALESCE(
                s.active_pid,
                (
                    SELECT pr.pid
                    FROM process_runs pr
                    WHERE pr.session_id = s.session_id
                    ORDER BY pr.run_id DESC
                    LIMIT 1
                )
            ) AS last_pid,
            NULL AS runtime_id,
            NULL AS runtime_label,
            NULL AS backend_class,
            COALESCE(
                (
                    SELECT st.workload
                    FROM session_turns st
                    WHERE st.session_id = s.session_id
                    ORDER BY st.turn_index DESC
                    LIMIT 1
                ),
                'general'
            ) AS workload,
            s.updated_at_ms,
            COALESCE(
                (
                    SELECT COUNT(*)
                    FROM session_turns st
                    WHERE st.session_id = s.session_id
                ),
                0
            ) AS turn_count,
            COALESCE(
                (
                    SELECT sm.content
                    FROM session_messages sm
                    JOIN session_turns st ON st.turn_id = sm.turn_id
                    WHERE sm.session_id = s.session_id
                      AND sm.role = 'user'
                    ORDER BY st.turn_index DESC, sm.ordinal ASC
                    LIMIT 1
                ),
                s.title
            ) AS prompt_preview
        FROM sessions s
        {filter}
        ORDER BY s.updated_at_ms DESC
        "#
    )
}

pub(crate) fn map_session_identity_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<SessionIdentity> {
    Ok(SessionIdentity {
        session_id: row.get(0)?,
        title: row.get(1)?,
        status: row.get(2)?,
        active_pid: row.get(3)?,
        last_pid: row.get(4)?,
        runtime_id: row.get(5)?,
        runtime_label: row.get(6)?,
        backend_class: row.get(7)?,
        workload: row.get(8)?,
        updated_at_ms: row.get(9)?,
        turn_count: row.get::<_, i64>(10)? as usize,
        prompt_preview: row.get(11)?,
    })
}

pub(crate) fn load_turns(
    connection: &Connection,
    session_id: &str,
) -> Result<Vec<StoredTurn>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT turn_id, turn_index, pid, workload, status, finish_reason
            FROM session_turns
            WHERE session_id = ?1
            ORDER BY turn_index ASC, turn_id ASC
            "#,
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![session_id], |row| {
            Ok(StoredTurn {
                turn_id: row.get(0)?,
                turn_index: row.get(1)?,
                pid: row.get(2)?,
                workload: row.get(3)?,
                status: row.get(4)?,
                finish_reason: row.get(5)?,
            })
        })
        .map_err(|err| err.to_string())?;

    let mut turns = Vec::new();
    for row in rows {
        turns.push(row.map_err(|err| err.to_string())?);
    }
    Ok(turns)
}

pub(crate) fn load_messages(
    connection: &Connection,
    session_id: &str,
) -> Result<Vec<StoredMessage>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT turn_id, ordinal, role, kind, content
            FROM session_messages
            WHERE session_id = ?1
            ORDER BY turn_id ASC, ordinal ASC
            "#,
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![session_id], |row| {
            Ok(StoredMessage {
                turn_id: row.get(0)?,
                ordinal: row.get(1)?,
                role: row.get(2)?,
                kind: row.get(3)?,
                content: row.get(4)?,
            })
        })
        .map_err(|err| err.to_string())?;

    let mut messages = Vec::new();
    for row in rows {
        messages.push(row.map_err(|err| err.to_string())?);
    }
    Ok(messages)
}

pub(crate) fn table_exists(connection: &Connection, table_name: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
            params![table_name],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .map_err(|err| err.to_string())
}

pub(crate) fn column_exists(
    connection: &Connection,
    table_name: &str,
    column_name: &str,
) -> Result<bool, String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table_name})"))
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| err.to_string())?;
    for row in rows {
        if row.map_err(|err| err.to_string())? == column_name {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn runtime_metadata_available(connection: &Connection) -> Result<bool, String> {
    Ok(table_exists(connection, "runtime_instances")?
        && column_exists(connection, "sessions", "runtime_id")?)
}

#[cfg(test)]
mod tests {
    use crate::kernel::history::{
        load_lobby_sessions, load_runtime_instances, load_runtime_load_queue,
        load_session_audit_events, load_timeline_seed, load_timeline_snapshot,
        load_workspace_snapshot,
    };
    use rusqlite::{params, Connection};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn lobby_sessions_include_archived_rows_from_sqlite() {
        let root = make_temp_root("agenticos-history-db");
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
                "#,
            )
            .expect("create schema");
        connection
            .execute(
                "INSERT INTO sessions VALUES (?1, ?2, 'idle', NULL, 1, 10)",
                params!["sess-1", "Archived session"],
            )
            .expect("insert session");
        connection
            .execute(
                "INSERT INTO process_runs (session_id, boot_id, pid, state, started_at_ms, ended_at_ms) VALUES (?1, 1, 33, 'completed', 1, 2)",
                params!["sess-1"],
            )
            .expect("insert process run");
        connection
            .execute(
                "INSERT INTO session_turns VALUES (1, ?1, 33, 1, 'general', 'legacy', 'completed', 1, 1, 2, 'completed')",
                params!["sess-1"],
            )
            .expect("insert turn");
        connection
            .execute(
                "INSERT INTO session_messages VALUES (1, ?1, 1, 33, 1, 'user', 'prompt', 'hello archive', 1)",
                params!["sess-1"],
            )
            .expect("insert message");

        let sessions = load_lobby_sessions(&root).expect("load lobby sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "sess-1");
        assert_eq!(sessions[0].pid, 33);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lobby_sessions_do_not_infer_pid_from_session_id_text() {
        let root = make_temp_root("agenticos-history-no-pid-inference");
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
                "#,
            )
            .expect("create schema");
        connection
            .execute(
                "INSERT INTO sessions VALUES (?1, ?2, 'idle', NULL, 1, 10)",
                params!["pid-999", "Archived pid-like session"],
            )
            .expect("insert session");

        let sessions = load_lobby_sessions(&root).expect("load lobby sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "pid-999");
        assert_eq!(sessions[0].pid, 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn timeline_snapshot_replays_user_and_assistant_messages() {
        let root = make_temp_root("agenticos-history-timeline");
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
                "#,
            )
            .expect("create schema");
        connection
            .execute(
                "INSERT INTO sessions VALUES (?1, ?2, 'idle', NULL, 1, 10)",
                params!["sess-2", "Archived session"],
            )
            .expect("insert session");
        connection
            .execute(
                "INSERT INTO process_runs (session_id, boot_id, pid, state, started_at_ms, ended_at_ms) VALUES (?1, 1, 44, 'completed', 1, 2)",
                params!["sess-2"],
            )
            .expect("insert process run");
        connection
            .execute(
                "INSERT INTO session_turns VALUES (1, ?1, 44, 1, 'general', 'legacy', 'completed', 1, 1, 2, 'completed')",
                params!["sess-2"],
            )
            .expect("insert turn");
        connection
            .execute(
                "INSERT INTO session_messages VALUES (1, ?1, 1, 44, 1, 'user', 'prompt', 'hello archive', 1)",
                params!["sess-2"],
            )
            .expect("insert user message");
        connection
            .execute(
                "INSERT INTO session_messages VALUES (2, ?1, 1, 44, 2, 'assistant', 'chunk', 'archived answer', 2)",
                params!["sess-2"],
            )
            .expect("insert assistant message");

        let timeline =
            load_timeline_snapshot(&root, "sess-2", Some(44)).expect("load timeline snapshot");
        let timeline = timeline.expect("timeline exists");
        assert_eq!(timeline.session_id, "sess-2");
        assert_eq!(timeline.pid, 44);
        assert_eq!(timeline.items.len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn timeline_seed_rehydrates_raw_turns_for_live_cache() {
        let root = make_temp_root("agenticos-history-seed");
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
                "#,
            )
            .expect("create schema");
        connection
            .execute(
                "INSERT INTO sessions VALUES (?1, ?2, 'idle', NULL, 1, 10)",
                params!["sess-seed", "Archived session"],
            )
            .expect("insert session");
        connection
            .execute(
                "INSERT INTO process_runs (session_id, boot_id, pid, state, started_at_ms, ended_at_ms) VALUES (?1, 1, 44, 'completed', 1, 2)",
                params!["sess-seed"],
            )
            .expect("insert process run");
        connection
            .execute(
                "INSERT INTO session_turns VALUES (1, ?1, 44, 1, 'general', 'legacy', 'completed', 1, 1, 2, 'completed')",
                params!["sess-seed"],
            )
            .expect("insert turn");
        connection
            .execute(
                "INSERT INTO session_messages VALUES (1, ?1, 1, 44, 1, 'user', 'prompt', 'hello archive', 1)",
                params!["sess-seed"],
            )
            .expect("insert user message");
        connection
            .execute(
                "INSERT INTO session_messages VALUES (2, ?1, 1, 44, 2, 'assistant', 'chunk', 'archived answer', 2)",
                params!["sess-seed"],
            )
            .expect("insert assistant message");

        let seed = load_timeline_seed(&root, "sess-seed", Some(77))
            .expect("load timeline seed")
            .expect("seed exists");
        assert_eq!(seed.session_id, "sess-seed");
        assert_eq!(seed.pid, 44);
        assert_eq!(seed.turns.len(), 1);
        assert_eq!(seed.turns[0].prompt, "hello archive");
        assert_eq!(seed.turns[0].messages.len(), 1);
        match &seed.turns[0].messages[0] {
            crate::kernel::live_timeline::TimelineSeedMessage::Assistant(text) => {
                assert_eq!(text, "archived answer")
            }
            other => panic!("unexpected seed message: {:?}", other),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_audit_events_are_loaded_from_sqlite() {
        let root = make_temp_root("agenticos-history-audit");
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
                CREATE TABLE audit_events (
                    audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    recorded_at_ms INTEGER NOT NULL,
                    category TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    title TEXT NOT NULL,
                    detail TEXT NOT NULL,
                    session_id TEXT NULL,
                    pid INTEGER NULL,
                    runtime_id TEXT NULL
                );
                "#,
            )
            .expect("create schema");
        connection
            .execute(
                "INSERT INTO sessions VALUES (?1, ?2, 'idle', NULL, 1, 10)",
                params!["sess-audit", "Audit session"],
            )
            .expect("insert session");
        connection
            .execute(
                "INSERT INTO audit_events (recorded_at_ms, category, kind, title, detail, session_id, pid, runtime_id) VALUES (1, 'process', 'spawned', 'Process spawned', 'pid=9', ?1, 9, 'rt-a')",
                params!["sess-audit"],
            )
            .expect("insert audit event");

        let events = load_session_audit_events(&root, "sess-audit", 16).expect("load audit");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "spawned");
        assert_eq!(events[0].pid, Some(9));
        assert_eq!(events[0].runtime_id.as_deref(), Some("rt-a"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_snapshot_keeps_audit_empty_when_no_real_events_exist() {
        let root = make_temp_root("agenticos-history-workspace-audit-empty");
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
                    runtime_id TEXT NULL,
                    active_pid INTEGER NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
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
                CREATE TABLE process_runs (
                    run_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    boot_id INTEGER NOT NULL,
                    pid INTEGER NOT NULL,
                    state TEXT NOT NULL,
                    started_at_ms INTEGER NOT NULL,
                    ended_at_ms INTEGER NULL
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
                "#,
            )
            .expect("create schema");
        connection
            .execute(
                "INSERT INTO sessions VALUES (?1, ?2, 'idle', NULL, NULL, 1, 10)",
                params!["sess-no-audit", "Auditless session"],
            )
            .expect("insert session");
        connection
            .execute(
                "INSERT INTO session_turns VALUES (1, ?1, 44, 1, 'general', 'legacy', 'completed', 1, 1, 2, 'completed')",
                params!["sess-no-audit"],
            )
            .expect("insert turn");

        let snapshot = load_workspace_snapshot(&root, "sess-no-audit", Some(44))
            .expect("load snapshot")
            .expect("snapshot exists");
        assert_eq!(snapshot.state, "Finished");
        assert!(snapshot.audit_events.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_snapshot_derives_waiting_for_input_for_interactive_sessions() {
        let root = make_temp_root("agenticos-history-workspace-waiting");
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
                    runtime_id TEXT NULL,
                    active_pid INTEGER NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
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
                CREATE TABLE process_runs (
                    run_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    boot_id INTEGER NOT NULL,
                    pid INTEGER NOT NULL,
                    state TEXT NOT NULL,
                    started_at_ms INTEGER NOT NULL,
                    ended_at_ms INTEGER NULL
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
                "#,
            )
            .expect("create schema");
        connection
            .execute(
                "INSERT INTO sessions VALUES (?1, ?2, 'idle', NULL, NULL, 1, 10)",
                params!["sess-waiting", "Waiting session"],
            )
            .expect("insert session");
        connection
            .execute(
                "INSERT INTO process_runs (session_id, boot_id, pid, state, started_at_ms, ended_at_ms) VALUES (?1, 1, 77, 'completed', 1, 2)",
                params!["sess-waiting"],
            )
            .expect("insert process run");
        connection
            .execute(
                "INSERT INTO session_turns VALUES (1, ?1, 77, 1, 'general', 'legacy', 'completed', 1, 1, 2, NULL)",
                params!["sess-waiting"],
            )
            .expect("insert turn");

        let snapshot = load_workspace_snapshot(&root, "sess-waiting", Some(77))
            .expect("load snapshot")
            .expect("snapshot exists");
        assert_eq!(snapshot.state, "WaitingForInput");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lobby_sessions_expose_runtime_metadata_and_queue() {
        let root = make_temp_root("agenticos-history-runtime");
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
                    runtime_id TEXT NULL,
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
                CREATE TABLE runtime_instances (
                    runtime_id TEXT PRIMARY KEY,
                    runtime_key TEXT NOT NULL,
                    state TEXT NOT NULL,
                    target_kind TEXT NOT NULL,
                    logical_model_id TEXT NOT NULL,
                    display_path TEXT NOT NULL,
                    runtime_reference TEXT NOT NULL,
                    family TEXT NOT NULL,
                    backend_id TEXT NOT NULL,
                    backend_class TEXT NOT NULL,
                    driver_source TEXT NOT NULL,
                    driver_rationale TEXT NOT NULL,
                    provider_id TEXT NULL,
                    remote_model_id TEXT NULL,
                    load_mode TEXT NOT NULL,
                    reservation_ram_bytes INTEGER NOT NULL,
                    reservation_vram_bytes INTEGER NOT NULL,
                    pinned INTEGER NOT NULL,
                    transition_state TEXT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    last_used_at_ms INTEGER NOT NULL
                );
                CREATE TABLE runtime_load_queue (
                    queue_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    runtime_key TEXT NOT NULL,
                    logical_model_id TEXT NOT NULL,
                    display_path TEXT NOT NULL,
                    backend_class TEXT NOT NULL,
                    state TEXT NOT NULL,
                    reservation_ram_bytes INTEGER NOT NULL,
                    reservation_vram_bytes INTEGER NOT NULL,
                    reason TEXT NOT NULL,
                    requested_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                "#,
            )
            .expect("create schema");
        connection
            .execute(
                "INSERT INTO runtime_instances VALUES (?1, 'key-1', 'registered', 'provider_model', 'gpt-4.1-mini', 'openai://gpt-4.1-mini', 'openai://gpt-4.1-mini', 'Chat', 'openai-compatible', 'remote_stateless', 'test', 'test', 'openai', 'gpt-4.1-mini', 'remote_stateless_adapter', 0, 0, 0, NULL, 1, 2, 3)",
                params!["rt-1"],
            )
            .expect("insert runtime");
        connection
            .execute(
                "INSERT INTO runtime_load_queue (runtime_key, logical_model_id, display_path, backend_class, state, reservation_ram_bytes, reservation_vram_bytes, reason, requested_at_ms, updated_at_ms) VALUES ('key-1', 'gpt-4.1-mini', 'openai://gpt-4.1-mini', 'remote_stateless', 'pending', 0, 0, 'waiting', 1, 2)",
                [],
            )
            .expect("insert queue");
        connection
            .execute(
                "INSERT INTO sessions VALUES (?1, ?2, 'idle', ?3, NULL, 1, 10)",
                params!["sess-rt", "Runtime session", "rt-1"],
            )
            .expect("insert session");
        connection
            .execute(
                "INSERT INTO process_runs (session_id, boot_id, pid, state, started_at_ms, ended_at_ms) VALUES (?1, 1, 55, 'completed', 1, 2)",
                params!["sess-rt"],
            )
            .expect("insert process run");
        connection
            .execute(
                "INSERT INTO session_turns VALUES (1, ?1, 55, 1, 'general', 'legacy', 'completed', 1, 1, 2, 'completed')",
                params!["sess-rt"],
            )
            .expect("insert turn");
        connection
            .execute(
                "INSERT INTO session_messages VALUES (1, ?1, 1, 55, 1, 'user', 'prompt', 'hello runtime', 1)",
                params!["sess-rt"],
            )
            .expect("insert message");

        let sessions = load_lobby_sessions(&root).expect("load sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].runtime_id.as_deref(), Some("rt-1"));
        assert_eq!(
            sessions[0].runtime_label.as_deref(),
            Some("openai · gpt-4.1-mini")
        );
        assert_eq!(
            sessions[0].backend_class.as_deref(),
            Some("remote_stateless")
        );

        let runtimes = load_runtime_instances(&root).expect("load runtimes");
        assert_eq!(runtimes.len(), 1);
        assert_eq!(runtimes[0].runtime_id, "rt-1");

        let queue = load_runtime_load_queue(&root).expect("load queue");
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].state, "pending");

        let _ = fs::remove_dir_all(root);
    }

    fn make_temp_root(prefix: &str) -> std::path::PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), timestamp));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
