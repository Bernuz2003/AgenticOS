use rusqlite::params;

use crate::storage::{current_timestamp_ms, StorageError, StorageService};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewDebugCheckpointRecord {
    pub(crate) session_id: Option<String>,
    pub(crate) pid: Option<u64>,
    pub(crate) runtime_id: Option<String>,
    pub(crate) boundary: String,
    pub(crate) state: String,
    pub(crate) snapshot_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredDebugCheckpointRecord {
    pub(crate) checkpoint_id: i64,
    pub(crate) recorded_at_ms: i64,
    pub(crate) session_id: Option<String>,
    pub(crate) pid: Option<u64>,
    pub(crate) runtime_id: Option<String>,
    pub(crate) boundary: String,
    pub(crate) state: String,
    pub(crate) snapshot_json: String,
}

impl StorageService {
    pub(crate) fn record_debug_checkpoint(
        &mut self,
        record: &NewDebugCheckpointRecord,
        retain: usize,
    ) -> Result<i64, StorageError> {
        let recorded_at_ms = current_timestamp_ms();
        self.connection.execute(
            r#"
            INSERT INTO debug_checkpoints (
                recorded_at_ms,
                session_id,
                pid,
                runtime_id,
                boundary,
                state,
                snapshot_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                recorded_at_ms,
                record.session_id,
                record.pid,
                record.runtime_id,
                record.boundary,
                record.state,
                record.snapshot_json,
            ],
        )?;

        let checkpoint_id = self.connection.last_insert_rowid();
        trim_debug_checkpoints(
            &self.connection,
            record.pid,
            record.session_id.as_deref(),
            retain.max(1),
        )?;
        Ok(checkpoint_id)
    }

    pub(crate) fn recent_debug_checkpoints_for_pid(
        &self,
        pid: u64,
        limit: usize,
    ) -> Result<Vec<StoredDebugCheckpointRecord>, StorageError> {
        let limit = limit.min(i64::MAX as usize) as i64;
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                checkpoint_id,
                recorded_at_ms,
                session_id,
                pid,
                runtime_id,
                boundary,
                state,
                snapshot_json
            FROM debug_checkpoints
            WHERE pid = ?1
            ORDER BY recorded_at_ms DESC, checkpoint_id DESC
            LIMIT ?2
            "#,
        )?;
        let rows = statement.query_map(params![pid, limit], map_debug_checkpoint_row)?;

        let mut values = Vec::new();
        for row in rows {
            values.push(row?);
        }
        Ok(values)
    }
}

fn trim_debug_checkpoints(
    connection: &rusqlite::Connection,
    pid: Option<u64>,
    session_id: Option<&str>,
    retain: usize,
) -> Result<(), rusqlite::Error> {
    let retain = retain.min(i64::MAX as usize) as i64;
    if let Some(pid) = pid {
        connection.execute(
            r#"
            DELETE FROM debug_checkpoints
            WHERE checkpoint_id IN (
                SELECT checkpoint_id
                FROM debug_checkpoints
                WHERE pid = ?1
                ORDER BY recorded_at_ms DESC, checkpoint_id DESC
                LIMIT -1 OFFSET ?2
            )
            "#,
            params![pid, retain],
        )?;
        return Ok(());
    }

    if let Some(session_id) = session_id {
        connection.execute(
            r#"
            DELETE FROM debug_checkpoints
            WHERE checkpoint_id IN (
                SELECT checkpoint_id
                FROM debug_checkpoints
                WHERE session_id = ?1
                ORDER BY recorded_at_ms DESC, checkpoint_id DESC
                LIMIT -1 OFFSET ?2
            )
            "#,
            params![session_id, retain],
        )?;
    }

    Ok(())
}

fn map_debug_checkpoint_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<StoredDebugCheckpointRecord> {
    Ok(StoredDebugCheckpointRecord {
        checkpoint_id: row.get(0)?,
        recorded_at_ms: row.get(1)?,
        session_id: row.get(2)?,
        pid: row.get(3)?,
        runtime_id: row.get(4)?,
        boundary: row.get(5)?,
        state: row.get(6)?,
        snapshot_json: row.get(7)?,
    })
}
