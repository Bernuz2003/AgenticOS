use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agentic_control_models::{
    BackendTelemetryView, RuntimeInstanceView, RuntimeLoadQueueEntryView,
};
use rusqlite::{params, Connection, OptionalExtension};

use super::stream::parse_stream_segments;
use crate::kernel::mapping::make_audit_event;
use crate::models::kernel::{
    AgentSessionSummary, AuditEvent, TimelineItem, TimelineItemKind, TimelineSnapshot,
    WorkspaceSnapshot,
};

#[derive(Debug)]
struct SessionIdentity {
    session_id: String,
    title: String,
    status: String,
    active_pid: Option<u64>,
    last_pid: Option<u64>,
    runtime_id: Option<String>,
    runtime_label: Option<String>,
    backend_class: Option<String>,
    workload: String,
    updated_at_ms: i64,
    turn_count: usize,
    prompt_preview: String,
}

#[derive(Debug)]
struct StoredTurn {
    turn_id: i64,
    turn_index: i64,
    pid: u64,
    workload: String,
    status: String,
    finish_reason: Option<String>,
}

#[derive(Debug)]
struct StoredMessage {
    turn_id: i64,
    ordinal: i64,
    role: String,
    kind: String,
    content: String,
}

#[derive(Debug)]
struct StoredAuditRow {
    recorded_at_ms: i64,
    category: String,
    kind: String,
    title: String,
    detail: String,
    session_id: Option<String>,
    pid: Option<u64>,
    runtime_id: Option<String>,
}

pub fn load_runtime_instances(workspace_root: &Path) -> Result<Vec<RuntimeInstanceView>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(Vec::new());
    };
    load_runtime_instances_from_connection(&connection)
}

pub fn load_runtime_load_queue(
    workspace_root: &Path,
) -> Result<Vec<RuntimeLoadQueueEntryView>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(Vec::new());
    };
    load_runtime_load_queue_from_connection(&connection)
}

pub fn load_lobby_sessions(workspace_root: &Path) -> Result<Vec<AgentSessionSummary>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(Vec::new());
    };
    load_all_session_identities(&connection)?
        .into_iter()
        .map(agent_session_summary_from_identity)
        .collect()
}

pub fn load_global_accounting_summary(
    workspace_root: &Path,
) -> Result<Option<BackendTelemetryView>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(None);
    };

    load_accounting_summary(&connection, None)
}

pub fn load_global_audit_events(
    workspace_root: &Path,
    limit: usize,
) -> Result<Vec<AuditEvent>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(Vec::new());
    };

    load_audit_events(&connection, None, limit)
}

pub fn load_session_audit_events(
    workspace_root: &Path,
    session_id: &str,
    limit: usize,
) -> Result<Vec<AuditEvent>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(Vec::new());
    };

    load_audit_events(&connection, Some(session_id), limit)
}

pub fn load_timeline_snapshot(
    workspace_root: &Path,
    session_id: &str,
    pid_hint: Option<u64>,
) -> Result<Option<TimelineSnapshot>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(None);
    };
    let Some(identity) = load_session_identity(&connection, session_id)? else {
        return Ok(None);
    };
    let turns = load_turns(&connection, &identity.session_id)?;
    let messages = load_messages(&connection, &identity.session_id)?;
    let running = turns
        .last()
        .is_some_and(|turn| matches!(turn.status.as_str(), "running" | "awaiting_turn_decision"));
    let pid = identity
        .active_pid
        .or(identity.last_pid)
        .or_else(|| turns.last().map(|turn| turn.pid))
        .or(pid_hint)
        .or_else(|| parse_pid_from_session_id(&identity.session_id))
        .unwrap_or(0);
    let workload = turns
        .last()
        .map(|turn| turn.workload.clone())
        .unwrap_or(identity.workload);
    let items = build_timeline_items(&identity.session_id, &turns, &messages);
    let error = messages
        .iter()
        .rev()
        .find(|message| message.kind == "error")
        .map(|message| message.content.clone());

    Ok(Some(TimelineSnapshot {
        session_id: identity.session_id,
        pid,
        running,
        workload,
        source: "sqlite_history".to_string(),
        fallback_notice: None,
        error,
        items,
    }))
}

pub fn load_workspace_snapshot(
    workspace_root: &Path,
    session_id: &str,
    pid_hint: Option<u64>,
) -> Result<Option<WorkspaceSnapshot>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(None);
    };
    let Some(identity) = load_session_identity(&connection, session_id)? else {
        return Ok(None);
    };
    let turns = load_turns(&connection, &identity.session_id)?;
    let workload = turns
        .last()
        .map(|turn| turn.workload.clone())
        .unwrap_or(identity.workload);
    let pid = identity
        .active_pid
        .or(identity.last_pid)
        .or_else(|| turns.last().map(|turn| turn.pid))
        .or(pid_hint)
        .or_else(|| parse_pid_from_session_id(&identity.session_id))
        .unwrap_or(0);
    let state = if identity.active_pid.is_some() {
        "Running".to_string()
    } else {
        "Idle".to_string()
    };
    let accounting = load_accounting_summary(&connection, Some(&identity.session_id))?;
    let mut audit_events = load_audit_events(&connection, Some(&identity.session_id), 64)?;
    if audit_events.is_empty() {
        audit_events = synthesize_workspace_audit_events(turns.last(), accounting.as_ref());
    }

    Ok(Some(WorkspaceSnapshot {
        session_id: identity.session_id,
        pid,
        active_pid: identity.active_pid,
        last_pid: identity.last_pid,
        title: identity.title,
        runtime_id: identity.runtime_id,
        runtime_label: identity.runtime_label,
        state,
        workload,
        context_slot_id: None,
        resident_slot_policy: None,
        resident_slot_state: None,
        resident_slot_snapshot_path: None,
        backend_id: None,
        backend_class: identity.backend_class,
        backend_capabilities: None,
        accounting,
        tokens_generated: 0,
        syscalls_used: 0,
        elapsed_secs: 0.0,
        tokens: 0,
        max_tokens: 0,
        orchestration: None,
        context: None,
        audit_events,
    }))
}

fn open_connection(workspace_root: &Path) -> Result<Option<Connection>, String> {
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

pub fn delete_session(workspace_root: &Path, session_id: &str) -> Result<(), String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(());
    };
    
    connection.execute("PRAGMA foreign_keys = ON", [])
        .map_err(|err| err.to_string())?;

    connection.execute(
        "DELETE FROM sessions WHERE session_id = ?1",
        params![session_id],
    )
    .map_err(|err| err.to_string())?;

    Ok(())
}

fn database_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join("workspace").join("agenticos.db")
}

fn load_all_session_identities(connection: &Connection) -> Result<Vec<SessionIdentity>, String> {
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

fn load_session_identity(
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

fn session_identity_select_query(filter_placeholder: Option<&str>) -> String {
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

fn session_identity_legacy_select_query(filter_placeholder: Option<&str>) -> String {
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

fn map_session_identity_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionIdentity> {
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

fn load_turns(connection: &Connection, session_id: &str) -> Result<Vec<StoredTurn>, String> {
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

fn load_messages(connection: &Connection, session_id: &str) -> Result<Vec<StoredMessage>, String> {
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

fn load_accounting_summary(
    connection: &Connection,
    session_id: Option<&str>,
) -> Result<Option<BackendTelemetryView>, String> {
    if !table_exists(connection, "accounting_events")? {
        return Ok(None);
    }

    let (summary_sql, last_model_sql, last_error_sql) = if session_id.is_some() {
        (
            r#"
            SELECT
                COUNT(*) AS event_count,
                COALESCE(SUM(request_count), 0),
                COALESCE(SUM(CASE WHEN stream != 0 THEN request_count ELSE 0 END), 0),
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(estimated_cost_usd), 0.0),
                COALESCE(SUM(CASE WHEN status = 'rate_limit_error' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'auth_error' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status IN ('transport_error', 'http_error') THEN 1 ELSE 0 END), 0)
            FROM accounting_events
            WHERE session_id = ?1
            "#,
            r#"
            SELECT model_id
            FROM accounting_events
            WHERE session_id = ?1
            ORDER BY recorded_at_ms DESC, event_id DESC
            LIMIT 1
            "#,
            r#"
            SELECT error_message
            FROM accounting_events
            WHERE session_id = ?1
              AND error_message IS NOT NULL
            ORDER BY recorded_at_ms DESC, event_id DESC
            LIMIT 1
            "#,
        )
    } else {
        (
            r#"
            SELECT
                COUNT(*) AS event_count,
                COALESCE(SUM(request_count), 0),
                COALESCE(SUM(CASE WHEN stream != 0 THEN request_count ELSE 0 END), 0),
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(estimated_cost_usd), 0.0),
                COALESCE(SUM(CASE WHEN status = 'rate_limit_error' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'auth_error' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status IN ('transport_error', 'http_error') THEN 1 ELSE 0 END), 0)
            FROM accounting_events
            "#,
            r#"
            SELECT model_id
            FROM accounting_events
            ORDER BY recorded_at_ms DESC, event_id DESC
            LIMIT 1
            "#,
            r#"
            SELECT error_message
            FROM accounting_events
            WHERE error_message IS NOT NULL
            ORDER BY recorded_at_ms DESC, event_id DESC
            LIMIT 1
            "#,
        )
    };

    let summary_row: (i64, i64, i64, i64, i64, f64, i64, i64, i64) =
        if let Some(session_id) = session_id {
            connection
                .query_row(summary_sql, params![session_id], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                })
                .map_err(|err| err.to_string())?
        } else {
            connection
                .query_row(summary_sql, [], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                })
                .map_err(|err| err.to_string())?
        };

    if summary_row.0 == 0 {
        return Ok(None);
    }

    let last_model = if let Some(session_id) = session_id {
        connection
            .query_row(last_model_sql, params![session_id], |row| row.get(0))
            .optional()
            .map_err(|err| err.to_string())?
    } else {
        connection
            .query_row(last_model_sql, [], |row| row.get(0))
            .optional()
            .map_err(|err| err.to_string())?
    };
    let last_error = if let Some(session_id) = session_id {
        connection
            .query_row(last_error_sql, params![session_id], |row| row.get(0))
            .optional()
            .map_err(|err| err.to_string())?
    } else {
        connection
            .query_row(last_error_sql, [], |row| row.get(0))
            .optional()
            .map_err(|err| err.to_string())?
    };

    Ok(Some(BackendTelemetryView {
        requests_total: summary_row.1.max(0) as u64,
        stream_requests_total: summary_row.2.max(0) as u64,
        input_tokens_total: summary_row.3.max(0) as u64,
        output_tokens_total: summary_row.4.max(0) as u64,
        estimated_cost_usd: summary_row.5,
        rate_limit_errors: summary_row.6.max(0) as u64,
        auth_errors: summary_row.7.max(0) as u64,
        transport_errors: summary_row.8.max(0) as u64,
        last_model,
        last_error,
    }))
}

fn load_audit_events(
    connection: &Connection,
    session_id: Option<&str>,
    limit: usize,
) -> Result<Vec<AuditEvent>, String> {
    if !table_exists(connection, "audit_events")? {
        return Ok(Vec::new());
    }

    let limit = limit.min(i64::MAX as usize) as i64;
    let mut statement = if session_id.is_some() {
        connection
            .prepare(
                r#"
                SELECT
                    recorded_at_ms,
                    category,
                    kind,
                    title,
                    detail,
                    session_id,
                    pid,
                    runtime_id
                FROM audit_events
                WHERE session_id = ?1
                ORDER BY recorded_at_ms DESC, audit_id DESC
                LIMIT ?2
                "#,
            )
            .map_err(|err| err.to_string())?
    } else {
        connection
            .prepare(
                r#"
                SELECT
                    recorded_at_ms,
                    category,
                    kind,
                    title,
                    detail,
                    session_id,
                    pid,
                    runtime_id
                FROM audit_events
                ORDER BY recorded_at_ms DESC, audit_id DESC
                LIMIT ?1
                "#,
            )
            .map_err(|err| err.to_string())?
    };

    let rows = if let Some(session_id) = session_id {
        statement
            .query_map(params![session_id, limit], map_audit_row)
            .map_err(|err| err.to_string())?
    } else {
        statement
            .query_map(params![limit], map_audit_row)
            .map_err(|err| err.to_string())?
    };

    let mut events = Vec::new();
    for row in rows {
        let row = row.map_err(|err| err.to_string())?;
        events.push(AuditEvent {
            category: row.category,
            kind: row.kind,
            title: row.title,
            detail: row.detail,
            recorded_at_ms: row.recorded_at_ms,
            session_id: row.session_id,
            pid: row.pid,
            runtime_id: row.runtime_id,
        });
    }

    Ok(events)
}

fn load_runtime_instances_from_connection(
    connection: &Connection,
) -> Result<Vec<RuntimeInstanceView>, String> {
    if !table_exists(connection, "runtime_instances")? {
        return Ok(Vec::new());
    }

    let mut statement = connection
        .prepare(
            r#"
            SELECT
                runtime_id,
                target_kind,
                logical_model_id,
                display_path,
                family,
                backend_id,
                backend_class,
                provider_id,
                remote_model_id,
                state,
                reservation_ram_bytes,
                reservation_vram_bytes,
                pinned,
                transition_state,
                0 AS active_pid_count,
                '' AS active_pids_json,
                0 AS current
            FROM runtime_instances
            ORDER BY updated_at_ms DESC, runtime_id ASC
            "#,
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(RuntimeInstanceView {
                runtime_id: row.get(0)?,
                target_kind: row.get(1)?,
                logical_model_id: row.get(2)?,
                display_path: row.get(3)?,
                family: row.get(4)?,
                backend_id: row.get(5)?,
                backend_class: row.get(6)?,
                provider_id: row.get(7)?,
                remote_model_id: row.get(8)?,
                state: row.get(9)?,
                reservation_ram_bytes: row.get(10)?,
                reservation_vram_bytes: row.get(11)?,
                pinned: row.get::<_, i64>(12)? != 0,
                transition_state: row.get(13)?,
                active_pid_count: row.get::<_, i64>(14)? as usize,
                active_pids: Vec::new(),
                current: row.get::<_, i64>(16)? != 0,
            })
        })
        .map_err(|err| err.to_string())?;

    let mut runtimes = Vec::new();
    for row in rows {
        runtimes.push(row.map_err(|err| err.to_string())?);
    }
    Ok(runtimes)
}

fn load_runtime_load_queue_from_connection(
    connection: &Connection,
) -> Result<Vec<RuntimeLoadQueueEntryView>, String> {
    if !table_exists(connection, "runtime_load_queue")? {
        return Ok(Vec::new());
    }

    let mut statement = connection
        .prepare(
            r#"
            SELECT
                queue_id,
                logical_model_id,
                display_path,
                backend_class,
                state,
                reservation_ram_bytes,
                reservation_vram_bytes,
                reason,
                requested_at_ms,
                updated_at_ms
            FROM runtime_load_queue
            ORDER BY requested_at_ms DESC, queue_id DESC
            LIMIT 32
            "#,
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(RuntimeLoadQueueEntryView {
                queue_id: row.get(0)?,
                logical_model_id: row.get(1)?,
                display_path: row.get(2)?,
                backend_class: row.get(3)?,
                state: row.get(4)?,
                reservation_ram_bytes: row.get(5)?,
                reservation_vram_bytes: row.get(6)?,
                reason: row.get(7)?,
                requested_at_ms: row.get(8)?,
                updated_at_ms: row.get(9)?,
            })
        })
        .map_err(|err| err.to_string())?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row.map_err(|err| err.to_string())?);
    }
    Ok(entries)
}

fn map_audit_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredAuditRow> {
    Ok(StoredAuditRow {
        recorded_at_ms: row.get(0)?,
        category: row.get(1)?,
        kind: row.get(2)?,
        title: row.get(3)?,
        detail: row.get(4)?,
        session_id: row.get(5)?,
        pid: row.get(6)?,
        runtime_id: row.get(7)?,
    })
}

fn agent_session_summary_from_identity(
    identity: SessionIdentity,
) -> Result<AgentSessionSummary, String> {
    let pid = identity
        .active_pid
        .or(identity.last_pid)
        .or_else(|| parse_pid_from_session_id(&identity.session_id))
        .unwrap_or(0);
    Ok(AgentSessionSummary {
        session_id: identity.session_id,
        pid,
        active_pid: identity.active_pid,
        last_pid: identity.last_pid,
        title: identity.title,
        prompt_preview: identity.prompt_preview,
        status: if identity.active_pid.is_some() || identity.status == "running" {
            "running".to_string()
        } else {
            "idle".to_string()
        },
        uptime_label: format_age_label(identity.updated_at_ms),
        tokens_label: if identity.turn_count == 0 {
            "0".to_string()
        } else {
            format!("{}t", identity.turn_count)
        },
        context_strategy: "sliding_window".to_string(),
        runtime_id: identity.runtime_id,
        runtime_label: identity.runtime_label,
        backend_class: identity.backend_class,
        orchestration_id: None,
        orchestration_task_id: None,
    })
}

fn table_exists(connection: &Connection, table_name: &str) -> Result<bool, String> {
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

fn column_exists(
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

fn runtime_metadata_available(connection: &Connection) -> Result<bool, String> {
    Ok(table_exists(connection, "runtime_instances")?
        && column_exists(connection, "sessions", "runtime_id")?)
}

fn synthesize_workspace_audit_events(
    last_turn: Option<&StoredTurn>,
    accounting: Option<&BackendTelemetryView>,
) -> Vec<AuditEvent> {
    let mut audit_events = Vec::new();
    if let Some(last_turn) = last_turn {
        if let Some(reason) = last_turn.finish_reason.as_ref() {
            audit_events.push(make_audit_event(
                "history",
                "Persisted turn state",
                format!("status={} reason={}", last_turn.status, reason),
            ));
        }
    }
    if let Some(accounting) = accounting {
        audit_events.push(make_audit_event(
            "accounting",
            "Persisted accounting",
            format!(
                "requests={} tokens={}/{} cost=${:.6} errors={}/{}/{}",
                accounting.requests_total,
                accounting.input_tokens_total,
                accounting.output_tokens_total,
                accounting.estimated_cost_usd,
                accounting.rate_limit_errors,
                accounting.auth_errors,
                accounting.transport_errors,
            ),
        ));
    }
    audit_events
}

fn build_timeline_items(
    session_id: &str,
    turns: &[StoredTurn],
    messages: &[StoredMessage],
) -> Vec<TimelineItem> {
    let mut grouped = BTreeMap::<i64, Vec<&StoredMessage>>::new();
    for message in messages {
        grouped.entry(message.turn_id).or_default().push(message);
    }

    let mut items = Vec::new();
    let mut system_index = 0usize;
    for turn in turns {
        let turn_id = format!("{}-turn-{}", session_id, turn.turn_index);
        let mut prompt = String::new();
        let mut assistant_stream = String::new();
        let mut system_messages = Vec::new();
        let running = matches!(turn.status.as_str(), "running" | "awaiting_turn_decision");

        if let Some(turn_messages) = grouped.get(&turn.turn_id) {
            let mut ordered = turn_messages.clone();
            ordered.sort_by_key(|message| message.ordinal);

            for message in ordered {
                match message.role.as_str() {
                    "user" if prompt.is_empty() => {
                        prompt = message.content.clone();
                    }
                    "assistant" => {
                        assistant_stream.push_str(&message.content);
                    }
                    "system" => {
                        system_messages.push((
                            message.content.clone(),
                            if message.kind == "error" {
                                "error".to_string()
                            } else {
                                "complete".to_string()
                            },
                        ));
                    }
                    _ => {}
                }
            }
        }

        if !prompt.is_empty() {
            items.push(TimelineItem {
                id: format!("{turn_id}-user"),
                kind: TimelineItemKind::UserMessage,
                text: prompt,
                status: "complete".to_string(),
            });
        }
        items.extend(parse_stream_segments(&turn_id, &assistant_stream, running));

        for (text, status) in system_messages {
            system_index += 1;
            items.push(TimelineItem {
                id: format!("{}-system-{}", session_id, system_index),
                kind: TimelineItemKind::SystemEvent,
                text,
                status,
            });
        }
    }

    items
}

fn format_age_label(updated_at_ms: i64) -> String {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(updated_at_ms);
    let delta_secs = ((now_ms - updated_at_ms).max(0) / 1_000) as u64;
    if delta_secs >= 86_400 {
        format!("{}d", delta_secs / 86_400)
    } else if delta_secs >= 3_600 {
        format!("{}h", delta_secs / 3_600)
    } else if delta_secs >= 60 {
        format!("{}m", delta_secs / 60)
    } else {
        format!("{}s", delta_secs)
    }
}

fn parse_pid_from_session_id(session_id: &str) -> Option<u64> {
    session_id.strip_prefix("pid-")?.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::{
        load_lobby_sessions, load_runtime_instances, load_runtime_load_queue,
        load_session_audit_events, load_timeline_snapshot,
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
