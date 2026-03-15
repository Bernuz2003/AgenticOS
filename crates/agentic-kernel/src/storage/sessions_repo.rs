use super::service::{current_timestamp_ms, StorageError, StorageService};
/// Session and PID lifecycle storage.
use rusqlite::params;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredSessionRecord {
    pub(crate) session_id: String,
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) runtime_id: Option<String>,
    pub(crate) active_pid: Option<u64>,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
}

impl StorageService {
    pub(crate) fn reset_active_sessions_for_boot(&mut self) -> Result<usize, StorageError> {
        Ok(self.connection.execute(
            r#"
            UPDATE sessions
            SET
                active_pid = NULL,
                status = CASE
                    WHEN status = 'running' THEN 'idle'
                    ELSE status
                END,
                updated_at_ms = ?1
            WHERE active_pid IS NOT NULL OR status = 'running'
            "#,
            params![current_timestamp_ms()],
        )?)
    }

    pub(crate) fn load_sessions(&self) -> Result<Vec<StoredSessionRecord>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT session_id, title, status, runtime_id, active_pid, created_at_ms, updated_at_ms
            FROM sessions
            ORDER BY created_at_ms ASC
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredSessionRecord {
                session_id: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                runtime_id: row.get(3)?,
                active_pid: row.get(4)?,
                created_at_ms: row.get(5)?,
                updated_at_ms: row.get(6)?,
            })
        })?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }

        Ok(sessions)
    }

    pub(crate) fn insert_session(
        &mut self,
        session_id: &str,
        title: &str,
        status: &str,
        runtime_id: Option<&str>,
        active_pid: Option<u64>,
        created_at_ms: i64,
        updated_at_ms: i64,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            r#"
            INSERT INTO sessions (
                session_id,
                title,
                status,
                runtime_id,
                active_pid,
                created_at_ms,
                updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                session_id,
                title,
                status,
                runtime_id,
                active_pid,
                created_at_ms,
                updated_at_ms
            ],
        )?;

        Ok(())
    }

    pub(crate) fn delete_session(&mut self, session_id: &str) -> Result<(), StorageError> {
        self.connection.execute(
            "DELETE FROM sessions WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    pub(crate) fn bind_session_to_pid(
        &mut self,
        session_id: &str,
        runtime_id: &str,
        boot_id: i64,
        pid: u64,
        started_at_ms: i64,
    ) -> Result<(), StorageError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            r#"
            UPDATE sessions
            SET active_pid = ?2, runtime_id = ?3, status = 'running', updated_at_ms = ?4
            WHERE session_id = ?1
            "#,
            params![session_id, pid, runtime_id, started_at_ms],
        )?;
        transaction.execute(
            r#"
            INSERT INTO process_runs (
                session_id,
                boot_id,
                pid,
                runtime_id,
                state,
                started_at_ms,
                ended_at_ms
            ) VALUES (?1, ?2, ?3, ?4, 'running', ?5, NULL)
            "#,
            params![session_id, boot_id, pid, runtime_id, started_at_ms],
        )?;
        transaction.commit()?;

        Ok(())
    }

    pub(crate) fn release_session_pid(
        &mut self,
        session_id: &str,
        boot_id: i64,
        pid: u64,
        run_state: &str,
        ended_at_ms: i64,
    ) -> Result<(), StorageError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            r#"
            UPDATE sessions
            SET active_pid = NULL, status = 'idle', updated_at_ms = ?2
            WHERE session_id = ?1 AND active_pid = ?4
            "#,
            params![session_id, ended_at_ms, pid],
        )?;
        transaction.execute(
            r#"
            UPDATE process_runs
            SET state = ?4, ended_at_ms = ?5
            WHERE session_id = ?1 AND boot_id = ?2 AND pid = ?3
            "#,
            params![session_id, boot_id, pid, run_state, ended_at_ms],
        )?;
        transaction.commit()?;

        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn session_by_id(
        &self,
        session_id: &str,
    ) -> Result<Option<StoredSessionRecord>, StorageError> {
        use rusqlite::OptionalExtension;

        Ok(self
            .connection
            .query_row(
                r#"
                SELECT session_id, title, status, runtime_id, active_pid, created_at_ms, updated_at_ms
                FROM sessions
                WHERE session_id = ?1
                "#,
                params![session_id],
                |row| {
                    Ok(StoredSessionRecord {
                        session_id: row.get(0)?,
                        title: row.get(1)?,
                        status: row.get(2)?,
                        runtime_id: row.get(3)?,
                        active_pid: row.get(4)?,
                        created_at_ms: row.get(5)?,
                        updated_at_ms: row.get(6)?,
                    })
                },
            )
            .optional()?)
    }
}
