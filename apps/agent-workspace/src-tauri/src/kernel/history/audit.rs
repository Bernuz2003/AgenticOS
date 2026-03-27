use std::path::Path;

use agentic_control_models::BackendTelemetryView;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::kernel::AuditEvent;

use super::db::{open_connection, table_exists, StoredAuditRow};

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

pub(crate) fn load_accounting_summary(
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

pub(crate) fn load_audit_events(
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
