use rusqlite::{params, OptionalExtension};

use crate::storage::{current_timestamp_ms, StorageError, StorageService};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewReplayBranchRecord {
    pub(crate) session_id: String,
    pub(crate) pid: u64,
    pub(crate) source_dump_id: String,
    pub(crate) source_session_id: Option<String>,
    pub(crate) source_pid: Option<u64>,
    pub(crate) source_fidelity: String,
    pub(crate) replay_mode: String,
    pub(crate) tool_mode: String,
    pub(crate) initial_state: String,
    pub(crate) patched_context_segments: usize,
    pub(crate) patched_episodic_segments: usize,
    pub(crate) stubbed_invocations: usize,
    pub(crate) overridden_invocations: usize,
    pub(crate) baseline_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredReplayBranchRecord {
    pub(crate) session_id: String,
    pub(crate) created_at_ms: i64,
    pub(crate) pid: u64,
    pub(crate) source_dump_id: String,
    pub(crate) source_session_id: Option<String>,
    pub(crate) source_pid: Option<u64>,
    pub(crate) source_fidelity: String,
    pub(crate) replay_mode: String,
    pub(crate) tool_mode: String,
    pub(crate) initial_state: String,
    pub(crate) patched_context_segments: usize,
    pub(crate) patched_episodic_segments: usize,
    pub(crate) stubbed_invocations: usize,
    pub(crate) overridden_invocations: usize,
    pub(crate) baseline_json: String,
}

impl StorageService {
    pub(crate) fn record_replay_branch(
        &mut self,
        record: &NewReplayBranchRecord,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            r#"
            INSERT INTO replay_branch_index (
                session_id,
                created_at_ms,
                pid,
                source_dump_id,
                source_session_id,
                source_pid,
                source_fidelity,
                replay_mode,
                tool_mode,
                initial_state,
                patched_context_segments,
                patched_episodic_segments,
                stubbed_invocations,
                overridden_invocations,
                baseline_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ON CONFLICT(session_id) DO UPDATE SET
                created_at_ms = excluded.created_at_ms,
                pid = excluded.pid,
                source_dump_id = excluded.source_dump_id,
                source_session_id = excluded.source_session_id,
                source_pid = excluded.source_pid,
                source_fidelity = excluded.source_fidelity,
                replay_mode = excluded.replay_mode,
                tool_mode = excluded.tool_mode,
                initial_state = excluded.initial_state,
                patched_context_segments = excluded.patched_context_segments,
                patched_episodic_segments = excluded.patched_episodic_segments,
                stubbed_invocations = excluded.stubbed_invocations,
                overridden_invocations = excluded.overridden_invocations,
                baseline_json = excluded.baseline_json
            "#,
            params![
                record.session_id,
                current_timestamp_ms(),
                record.pid,
                record.source_dump_id,
                record.source_session_id,
                record.source_pid,
                record.source_fidelity,
                record.replay_mode,
                record.tool_mode,
                record.initial_state,
                record.patched_context_segments.min(i64::MAX as usize) as i64,
                record.patched_episodic_segments.min(i64::MAX as usize) as i64,
                record.stubbed_invocations.min(i64::MAX as usize) as i64,
                record.overridden_invocations.min(i64::MAX as usize) as i64,
                record.baseline_json,
            ],
        )?;
        Ok(())
    }

    pub(crate) fn replay_branch(
        &self,
        session_id: &str,
    ) -> Result<Option<StoredReplayBranchRecord>, StorageError> {
        self.connection
            .query_row(
                r#"
                SELECT
                    session_id,
                    created_at_ms,
                    pid,
                    source_dump_id,
                    source_session_id,
                    source_pid,
                    source_fidelity,
                    replay_mode,
                    tool_mode,
                    initial_state,
                    patched_context_segments,
                    patched_episodic_segments,
                    stubbed_invocations,
                    overridden_invocations,
                    baseline_json
                FROM replay_branch_index
                WHERE session_id = ?1
                "#,
                params![session_id],
                |row| {
                    Ok(StoredReplayBranchRecord {
                        session_id: row.get(0)?,
                        created_at_ms: row.get(1)?,
                        pid: row.get(2)?,
                        source_dump_id: row.get(3)?,
                        source_session_id: row.get(4)?,
                        source_pid: row.get(5)?,
                        source_fidelity: row.get(6)?,
                        replay_mode: row.get(7)?,
                        tool_mode: row.get(8)?,
                        initial_state: row.get(9)?,
                        patched_context_segments: row.get::<_, i64>(10)? as usize,
                        patched_episodic_segments: row.get::<_, i64>(11)? as usize,
                        stubbed_invocations: row.get::<_, i64>(12)? as usize,
                        overridden_invocations: row.get::<_, i64>(13)? as usize,
                        baseline_json: row.get(14)?,
                    })
                },
            )
            .optional()
            .map_err(StorageError::from)
    }
}
