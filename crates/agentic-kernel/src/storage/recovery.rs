use std::collections::HashSet;

use rusqlite::{params, Transaction};

use super::service::{current_timestamp_ms, StorageError, StorageService};
use super::timeline::{insert_message, next_message_ordinal};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BootRecoveryReport {
    pub(crate) recovered_at_ms: i64,
    pub(crate) stale_active_sessions_reset: usize,
    pub(crate) interrupted_process_runs: usize,
    pub(crate) interrupted_turns: usize,
    pub(crate) interrupted_scheduler_job_runs: usize,
    pub(crate) logical_resume_sessions: usize,
    pub(crate) strong_restore_candidate_sessions: usize,
    pub(crate) pending_runtime_queue_entries: usize,
    pub(crate) persisted_sessions: usize,
    pub(crate) known_runtimes: usize,
}

#[derive(Debug)]
struct InterruptedTurnCandidate {
    turn_id: i64,
    session_id: String,
    pid: u64,
    previous_status: String,
    backend_class: Option<String>,
    backend_id: Option<String>,
}

impl InterruptedTurnCandidate {
    fn supports_strong_restore(&self) -> bool {
        matches!(self.backend_class.as_deref(), Some("resident_local"))
    }
}

impl StorageService {
    pub(crate) fn run_boot_recovery(&mut self) -> Result<BootRecoveryReport, StorageError> {
        let recovered_at_ms = current_timestamp_ms();
        let transaction = self.connection.transaction()?;
        let candidates = load_interrupted_turn_candidates(&transaction)?;
        let interrupted_process_runs =
            mark_interrupted_process_runs(&transaction, recovered_at_ms)?;
        let stale_active_sessions_reset =
            reset_active_sessions_for_boot_tx(&transaction, recovered_at_ms)?;
        let interrupted_scheduler_job_runs =
            mark_interrupted_scheduler_job_runs(&transaction, recovered_at_ms)?;
        let (interrupted_turns, logical_resume_sessions, strong_restore_candidate_sessions) =
            mark_interrupted_turns(&transaction, recovered_at_ms, &candidates)?;
        let pending_runtime_queue_entries = count_rows(
            &transaction,
            "SELECT COUNT(*) FROM runtime_load_queue WHERE state = 'pending'",
        )?;
        let persisted_sessions = count_rows(&transaction, "SELECT COUNT(*) FROM sessions")?;
        let known_runtimes = count_rows(&transaction, "SELECT COUNT(*) FROM runtime_instances")?;
        transaction.commit()?;

        Ok(BootRecoveryReport {
            recovered_at_ms,
            stale_active_sessions_reset,
            interrupted_process_runs,
            interrupted_turns,
            interrupted_scheduler_job_runs,
            logical_resume_sessions,
            strong_restore_candidate_sessions,
            pending_runtime_queue_entries,
            persisted_sessions,
            known_runtimes,
        })
    }
}

fn mark_interrupted_process_runs(
    transaction: &Transaction<'_>,
    recovered_at_ms: i64,
) -> Result<usize, rusqlite::Error> {
    transaction.execute(
        r#"
        UPDATE process_runs
        SET
            state = 'interrupted',
            ended_at_ms = COALESCE(ended_at_ms, ?1)
        WHERE ended_at_ms IS NULL
        "#,
        params![recovered_at_ms],
    )
}

fn reset_active_sessions_for_boot_tx(
    transaction: &Transaction<'_>,
    recovered_at_ms: i64,
) -> Result<usize, rusqlite::Error> {
    transaction.execute(
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
        params![recovered_at_ms],
    )
}

fn mark_interrupted_scheduler_job_runs(
    transaction: &Transaction<'_>,
    recovered_at_ms: i64,
) -> Result<usize, rusqlite::Error> {
    transaction.execute(
        r#"
        UPDATE scheduled_job_runs
        SET
            status = 'interrupted',
            completed_at_ms = COALESCE(completed_at_ms, ?1),
            error = CASE
                WHEN error IS NULL OR error = '' THEN 'kernel_restarted'
                ELSE error
            END
        WHERE status = 'running' AND completed_at_ms IS NULL
        "#,
        params![recovered_at_ms],
    )
}

fn mark_interrupted_turns(
    transaction: &Transaction<'_>,
    recovered_at_ms: i64,
    candidates: &[InterruptedTurnCandidate],
) -> Result<(usize, usize, usize), rusqlite::Error> {
    let mut logical_resume_sessions = HashSet::<String>::new();
    let mut strong_restore_candidate_sessions = HashSet::<String>::new();

    for candidate in candidates {
        transaction.execute(
            r#"
            UPDATE session_turns
            SET
                status = 'interrupted',
                updated_at_ms = ?2,
                completed_at_ms = COALESCE(completed_at_ms, ?2),
                finish_reason = 'kernel_restarted'
            WHERE turn_id = ?1
            "#,
            params![candidate.turn_id, recovered_at_ms],
        )?;
        let ordinal = next_message_ordinal(transaction, candidate.turn_id)?;
        insert_message(
            transaction,
            &candidate.session_id,
            candidate.turn_id,
            candidate.pid,
            ordinal,
            "system",
            "marker",
            &recovery_marker_text(candidate),
            recovered_at_ms,
        )?;
        transaction.execute(
            "UPDATE sessions SET updated_at_ms = ?2 WHERE session_id = ?1",
            params![candidate.session_id, recovered_at_ms],
        )?;

        if candidate.supports_strong_restore() {
            strong_restore_candidate_sessions.insert(candidate.session_id.clone());
        } else {
            logical_resume_sessions.insert(candidate.session_id.clone());
        }
    }

    Ok((
        candidates.len(),
        logical_resume_sessions.len(),
        strong_restore_candidate_sessions.len(),
    ))
}

fn load_interrupted_turn_candidates(
    transaction: &Transaction<'_>,
) -> Result<Vec<InterruptedTurnCandidate>, rusqlite::Error> {
    let mut statement = transaction.prepare(
        r#"
        SELECT
            st.turn_id,
            st.session_id,
            st.pid,
            st.status,
            ri.backend_class,
            ri.backend_id
        FROM session_turns st
        JOIN sessions s ON s.session_id = st.session_id
        LEFT JOIN runtime_instances ri ON ri.runtime_id = s.runtime_id
        WHERE st.completed_at_ms IS NULL
          AND st.status IN ('running', 'awaiting_turn_decision')
        ORDER BY st.turn_id ASC
        "#,
    )?;
    let rows = statement.query_map([], |row| {
        Ok(InterruptedTurnCandidate {
            turn_id: row.get(0)?,
            session_id: row.get(1)?,
            pid: row.get(2)?,
            previous_status: row.get(3)?,
            backend_class: row.get(4)?,
            backend_id: row.get(5)?,
        })
    })?;

    let mut candidates = Vec::new();
    for row in rows {
        candidates.push(row?);
    }
    Ok(candidates)
}

fn recovery_marker_text(candidate: &InterruptedTurnCandidate) -> String {
    let mut detail = match candidate.backend_class.as_deref() {
        Some("resident_local") => format!(
            "Kernel rebooted. Session resumed logically from SQLite history; backend '{}' remains a strong-restore candidate only where resident slot restore is supported.",
            candidate.backend_id.as_deref().unwrap_or("resident_local")
        ),
        Some("remote_stateless") => format!(
            "Kernel rebooted. Session resumed logically from SQLite history; backend '{}' cannot restore the live process and must continue from persisted context only.",
            candidate.backend_id.as_deref().unwrap_or("remote_stateless")
        ),
        _ => "Kernel rebooted. Session resumed logically from SQLite history; no live process restore was attempted.".to_string(),
    };

    if candidate.previous_status == "awaiting_turn_decision" {
        detail.push_str(
            " The previous truncated turn cannot be continued after reboot; resume with a new input.",
        );
    }

    detail
}

fn count_rows(transaction: &Transaction<'_>, sql: &str) -> Result<usize, rusqlite::Error> {
    transaction
        .query_row(sql, [], |row| row.get::<_, i64>(0))
        .map(|count| count.max(0) as usize)
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
