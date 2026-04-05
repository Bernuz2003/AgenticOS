use rusqlite::params;

use crate::storage::{current_timestamp_ms, StorageError, StorageService};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewToolInvocationRecord {
    pub(crate) tool_call_id: String,
    pub(crate) session_id: Option<String>,
    pub(crate) pid: Option<u64>,
    pub(crate) runtime_id: Option<String>,
    pub(crate) tool_name: String,
    pub(crate) caller: String,
    pub(crate) transport: String,
    pub(crate) status: String,
    pub(crate) command_text: String,
    pub(crate) input_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletedToolInvocationRecord {
    pub(crate) tool_call_id: String,
    pub(crate) status: String,
    pub(crate) output_json: Option<String>,
    pub(crate) output_text: Option<String>,
    pub(crate) warnings_json: Option<String>,
    pub(crate) error_kind: Option<String>,
    pub(crate) error_text: Option<String>,
    pub(crate) effect_json: Option<String>,
    pub(crate) duration_ms: Option<u128>,
    pub(crate) kill: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredToolInvocationRecord {
    pub(crate) invocation_id: i64,
    pub(crate) tool_call_id: String,
    pub(crate) recorded_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) session_id: Option<String>,
    pub(crate) pid: Option<u64>,
    pub(crate) runtime_id: Option<String>,
    pub(crate) tool_name: String,
    pub(crate) caller: String,
    pub(crate) transport: String,
    pub(crate) status: String,
    pub(crate) command_text: String,
    pub(crate) input_json: String,
    pub(crate) output_json: Option<String>,
    pub(crate) output_text: Option<String>,
    pub(crate) warnings_json: Option<String>,
    pub(crate) error_kind: Option<String>,
    pub(crate) error_text: Option<String>,
    pub(crate) effect_json: Option<String>,
    pub(crate) duration_ms: Option<u128>,
    pub(crate) kill: bool,
}

impl StorageService {
    pub(crate) fn record_tool_invocation_dispatch(
        &mut self,
        record: &NewToolInvocationRecord,
        retain: usize,
    ) -> Result<(), StorageError> {
        let recorded_at_ms = current_timestamp_ms();
        self.connection.execute(
            r#"
            INSERT INTO tool_invocation_history (
                tool_call_id,
                recorded_at_ms,
                updated_at_ms,
                session_id,
                pid,
                runtime_id,
                tool_name,
                caller,
                transport,
                status,
                command_text,
                input_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
            params![
                record.tool_call_id,
                recorded_at_ms,
                recorded_at_ms,
                record.session_id,
                record.pid,
                record.runtime_id,
                record.tool_name,
                record.caller,
                record.transport,
                record.status,
                record.command_text,
                record.input_json,
            ],
        )?;

        trim_tool_invocations(
            &self.connection,
            record.pid,
            record.session_id.as_deref(),
            retain.max(1),
        )?;
        Ok(())
    }

    pub(crate) fn complete_tool_invocation(
        &mut self,
        record: &CompletedToolInvocationRecord,
    ) -> Result<(), StorageError> {
        let updated_at_ms = current_timestamp_ms();
        self.connection.execute(
            r#"
            UPDATE tool_invocation_history
            SET
                updated_at_ms = ?2,
                status = ?3,
                output_json = ?4,
                output_text = ?5,
                warnings_json = ?6,
                error_kind = ?7,
                error_text = ?8,
                effect_json = ?9,
                duration_ms = ?10,
                kill = ?11
            WHERE tool_call_id = ?1
            "#,
            params![
                record.tool_call_id,
                updated_at_ms,
                record.status,
                record.output_json,
                record.output_text,
                record.warnings_json,
                record.error_kind,
                record.error_text,
                record.effect_json,
                record
                    .duration_ms
                    .map(|value| value.min(i64::MAX as u128) as i64),
                if record.kill { 1 } else { 0 },
            ],
        )?;
        Ok(())
    }

    pub(crate) fn recent_tool_invocations_for_pid(
        &self,
        pid: u64,
        limit: usize,
    ) -> Result<Vec<StoredToolInvocationRecord>, StorageError> {
        let limit = limit.min(i64::MAX as usize) as i64;
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                invocation_id,
                tool_call_id,
                recorded_at_ms,
                updated_at_ms,
                session_id,
                pid,
                runtime_id,
                tool_name,
                caller,
                transport,
                status,
                command_text,
                input_json,
                output_json,
                output_text,
                warnings_json,
                error_kind,
                error_text,
                effect_json,
                duration_ms,
                kill
            FROM tool_invocation_history
            WHERE pid = ?1
            ORDER BY recorded_at_ms DESC, invocation_id DESC
            LIMIT ?2
            "#,
        )?;
        let rows = statement.query_map(params![pid, limit], map_tool_invocation_row)?;

        let mut values = Vec::new();
        for row in rows {
            values.push(row?);
        }
        Ok(values)
    }
}

fn trim_tool_invocations(
    connection: &rusqlite::Connection,
    pid: Option<u64>,
    session_id: Option<&str>,
    retain: usize,
) -> Result<(), rusqlite::Error> {
    let retain = retain.min(i64::MAX as usize) as i64;
    if let Some(pid) = pid {
        connection.execute(
            r#"
            DELETE FROM tool_invocation_history
            WHERE invocation_id IN (
                SELECT invocation_id
                FROM tool_invocation_history
                WHERE pid = ?1
                ORDER BY recorded_at_ms DESC, invocation_id DESC
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
            DELETE FROM tool_invocation_history
            WHERE invocation_id IN (
                SELECT invocation_id
                FROM tool_invocation_history
                WHERE session_id = ?1
                ORDER BY recorded_at_ms DESC, invocation_id DESC
                LIMIT -1 OFFSET ?2
            )
            "#,
            params![session_id, retain],
        )?;
    }

    Ok(())
}

fn map_tool_invocation_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<StoredToolInvocationRecord> {
    Ok(StoredToolInvocationRecord {
        invocation_id: row.get(0)?,
        tool_call_id: row.get(1)?,
        recorded_at_ms: row.get(2)?,
        updated_at_ms: row.get(3)?,
        session_id: row.get(4)?,
        pid: row.get(5)?,
        runtime_id: row.get(6)?,
        tool_name: row.get(7)?,
        caller: row.get(8)?,
        transport: row.get(9)?,
        status: row.get(10)?,
        command_text: row.get(11)?,
        input_json: row.get(12)?,
        output_json: row.get(13)?,
        output_text: row.get(14)?,
        warnings_json: row.get(15)?,
        error_kind: row.get(16)?,
        error_text: row.get(17)?,
        effect_json: row.get(18)?,
        duration_ms: row
            .get::<_, Option<i64>>(19)?
            .map(|value| value.max(0) as u128),
        kill: row.get::<_, i64>(20)? != 0,
    })
}
