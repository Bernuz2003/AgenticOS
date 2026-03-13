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
mod tests {
    use super::BootRecoveryReport;
    use crate::storage::StorageService;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn boot_recovery_interrupts_open_runs_and_turns() {
        let dir = make_temp_dir("agenticos-boot-recovery");
        let db_path = dir.join("agenticos.db");

        let mut storage = StorageService::open(&db_path).expect("open storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record boot");
        storage
            .insert_session(
                "sess-1",
                "Recovered session",
                "running",
                Some("rt-local"),
                Some(7),
                1_000,
                1_000,
            )
            .expect("insert session");
        storage
            .upsert_runtime_instance(&crate::storage::StoredRuntimeRecord {
                runtime_id: "rt-local".to_string(),
                runtime_key: "local-key".to_string(),
                state: "registered".to_string(),
                target_kind: "model".to_string(),
                logical_model_id: "qwen".to_string(),
                display_path: "/tmp/qwen.gguf".to_string(),
                runtime_reference: "/tmp/qwen.gguf".to_string(),
                family: "Qwen".to_string(),
                backend_id: "external-llamacpp".to_string(),
                backend_class: "resident_local".to_string(),
                driver_source: "test".to_string(),
                driver_rationale: "test".to_string(),
                provider_id: None,
                remote_model_id: None,
                load_mode: "resident_local_adapter".to_string(),
                reservation_ram_bytes: 1,
                reservation_vram_bytes: 1,
                pinned: false,
                transition_state: None,
                created_at_ms: 1_000,
                updated_at_ms: 1_000,
                last_used_at_ms: 1_000,
            })
            .expect("insert runtime");
        storage
            .bind_session_to_pid("sess-1", "rt-local", boot.boot_id, 7, 2_000)
            .expect("bind session");
        storage
            .start_session_turn("sess-1", 7, "general", "exec", "hello", "prompt")
            .expect("start turn");

        let report = storage.run_boot_recovery().expect("run boot recovery");

        assert_eq!(
            report,
            BootRecoveryReport {
                recovered_at_ms: report.recovered_at_ms,
                stale_active_sessions_reset: 1,
                interrupted_process_runs: 1,
                interrupted_turns: 1,
                logical_resume_sessions: 0,
                strong_restore_candidate_sessions: 1,
                pending_runtime_queue_entries: 0,
                persisted_sessions: 1,
                known_runtimes: 1,
            }
        );

        let session: (Option<u64>, String) = storage
            .connection
            .query_row(
                "SELECT active_pid, status FROM sessions WHERE session_id = 'sess-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load session");
        assert_eq!(session.0, None);
        assert_eq!(session.1, "idle");

        let run: (String, Option<i64>) = storage
            .connection
            .query_row(
                "SELECT state, ended_at_ms FROM process_runs WHERE session_id = 'sess-1' LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load process run");
        assert_eq!(run.0, "interrupted");
        assert!(run.1.is_some());

        let turn: (String, String) = storage
            .connection
            .query_row(
                "SELECT status, finish_reason FROM session_turns WHERE session_id = 'sess-1' LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("load turn");
        assert_eq!(turn.0, "interrupted");
        assert_eq!(turn.1, "kernel_restarted");

        let marker: String = storage
            .connection
            .query_row(
                r#"
                SELECT content
                FROM session_messages
                WHERE session_id = 'sess-1' AND role = 'system'
                ORDER BY message_id DESC
                LIMIT 1
                "#,
                [],
                |row| row.get(0),
            )
            .expect("load marker");
        assert!(marker.contains("strong-restore candidate"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn boot_recovery_reports_pending_runtime_queue_entries() {
        let dir = make_temp_dir("agenticos-boot-recovery-queue");
        let db_path = dir.join("agenticos.db");

        let mut storage = StorageService::open(&db_path).expect("open storage");
        storage
            .insert_runtime_load_queue_entry(
                "rt-1",
                "qwen",
                "/tmp/qwen.gguf",
                "resident_local",
                "pending",
                10,
                20,
                "waiting for VRAM",
            )
            .expect("insert pending entry");
        storage
            .insert_runtime_load_queue_entry(
                "rt-2",
                "gpt-4.1-mini",
                "openai://gpt-4.1-mini",
                "remote_stateless",
                "admitted",
                0,
                0,
                "already admitted",
            )
            .expect("insert admitted entry");

        let report = storage.run_boot_recovery().expect("run boot recovery");
        assert_eq!(report.pending_runtime_queue_entries, 1);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn boot_recovery_marks_remote_stateless_sessions_as_logical_resume_only() {
        let dir = make_temp_dir("agenticos-boot-recovery-remote");
        let db_path = dir.join("agenticos.db");

        let mut storage = StorageService::open(&db_path).expect("open storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record boot");
        storage
            .insert_session(
                "sess-remote",
                "Recovered remote session",
                "running",
                Some("rt-remote"),
                Some(11),
                1_000,
                1_000,
            )
            .expect("insert session");
        storage
            .upsert_runtime_instance(&crate::storage::StoredRuntimeRecord {
                runtime_id: "rt-remote".to_string(),
                runtime_key: "remote-key".to_string(),
                state: "registered".to_string(),
                target_kind: "provider_model".to_string(),
                logical_model_id: "gpt-4.1-mini".to_string(),
                display_path: "openai://gpt-4.1-mini".to_string(),
                runtime_reference: "openai://gpt-4.1-mini".to_string(),
                family: "Chat".to_string(),
                backend_id: "openai-compatible".to_string(),
                backend_class: "remote_stateless".to_string(),
                driver_source: "test".to_string(),
                driver_rationale: "test".to_string(),
                provider_id: Some("openai".to_string()),
                remote_model_id: Some("gpt-4.1-mini".to_string()),
                load_mode: "remote_stateless_adapter".to_string(),
                reservation_ram_bytes: 0,
                reservation_vram_bytes: 0,
                pinned: false,
                transition_state: None,
                created_at_ms: 1_000,
                updated_at_ms: 1_000,
                last_used_at_ms: 1_000,
            })
            .expect("insert runtime");
        storage
            .bind_session_to_pid("sess-remote", "rt-remote", boot.boot_id, 11, 2_000)
            .expect("bind session");
        storage
            .start_session_turn(
                "sess-remote",
                11,
                "general",
                "exec",
                "hello remote",
                "prompt",
            )
            .expect("start turn");

        let report = storage.run_boot_recovery().expect("run boot recovery");

        assert_eq!(report.interrupted_turns, 1);
        assert_eq!(report.logical_resume_sessions, 1);
        assert_eq!(report.strong_restore_candidate_sessions, 0);

        let marker: String = storage
            .connection
            .query_row(
                r#"
                SELECT content
                FROM session_messages
                WHERE session_id = 'sess-remote' AND role = 'system'
                ORDER BY message_id DESC
                LIMIT 1
                "#,
                [],
                |row| row.get(0),
            )
            .expect("load marker");
        assert!(marker.contains("cannot restore the live process"));
        assert!(marker.contains("persisted context only"));

        let _ = fs::remove_dir_all(dir);
    }

    fn make_temp_dir(prefix: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{timestamp}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
