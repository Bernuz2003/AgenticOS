use rusqlite::params;

use super::service::{current_timestamp_ms, StorageError, StorageService};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewAuditEvent {
    pub(crate) category: String,
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) session_id: Option<String>,
    pub(crate) pid: Option<u64>,
    pub(crate) runtime_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredAuditEvent {
    pub(crate) audit_id: i64,
    pub(crate) recorded_at_ms: i64,
    pub(crate) category: String,
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) session_id: Option<String>,
    pub(crate) pid: Option<u64>,
    pub(crate) runtime_id: Option<String>,
}

impl StorageService {
    pub(crate) fn record_audit_event(
        &mut self,
        event: &NewAuditEvent,
    ) -> Result<i64, StorageError> {
        let recorded_at_ms = current_timestamp_ms();
        self.connection.execute(
            r#"
            INSERT INTO audit_events (
                recorded_at_ms,
                category,
                kind,
                title,
                detail,
                session_id,
                pid,
                runtime_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                recorded_at_ms,
                event.category,
                event.kind,
                event.title,
                event.detail,
                event.session_id,
                event.pid,
                event.runtime_id,
            ],
        )?;

        Ok(recorded_at_ms)
    }

    #[allow(dead_code)]
    pub(crate) fn recent_audit_events(
        &self,
        limit: usize,
    ) -> Result<Vec<StoredAuditEvent>, StorageError> {
        load_audit_events(&self.connection, None, limit)
    }

    #[allow(dead_code)]
    pub(crate) fn recent_audit_events_for_session(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredAuditEvent>, StorageError> {
        load_audit_events(&self.connection, Some(session_id), limit)
    }

    #[cfg(test)]
    pub(crate) fn audit_event_count(&self) -> Result<i64, StorageError> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM audit_events", [], |row| row.get(0))?)
    }
}

#[allow(dead_code)]
fn load_audit_events(
    connection: &rusqlite::Connection,
    session_id: Option<&str>,
    limit: usize,
) -> Result<Vec<StoredAuditEvent>, StorageError> {
    let limit = limit.min(i64::MAX as usize) as i64;
    let mut statement = if session_id.is_some() {
        connection.prepare(
            r#"
            SELECT
                audit_id,
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
        )?
    } else {
        connection.prepare(
            r#"
            SELECT
                audit_id,
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
        )?
    };

    let rows = if let Some(session_id) = session_id {
        statement.query_map(params![session_id, limit], map_audit_row)?
    } else {
        statement.query_map(params![limit], map_audit_row)?
    };

    let mut events = Vec::new();
    for row in rows {
        events.push(row?);
    }
    Ok(events)
}

#[allow(dead_code)]
fn map_audit_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredAuditEvent> {
    Ok(StoredAuditEvent {
        audit_id: row.get(0)?,
        recorded_at_ms: row.get(1)?,
        category: row.get(2)?,
        kind: row.get(3)?,
        title: row.get(4)?,
        detail: row.get(5)?,
        session_id: row.get(6)?,
        pid: row.get(7)?,
        runtime_id: row.get(8)?,
    })
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod tests;
