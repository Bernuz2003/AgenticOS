use std::fs;
use std::path::Path;

use rusqlite::{params, OptionalExtension, Transaction};
use serde::Deserialize;

use super::kernel_repo::upsert_kernel_meta;
use super::service::{current_timestamp_ms, StorageError, StorageService};

const LEGACY_IMPORT_META_KEY: &str = "legacy_timeline_import_v1_completed_at_ms";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LegacyTimelineImportReport {
    pub(crate) imported_sessions: usize,
    pub(crate) imported_turns: usize,
    pub(crate) imported_messages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredReplayMessage {
    pub(crate) role: String,
    pub(crate) kind: String,
    pub(crate) content: String,
}

#[derive(Debug, Deserialize)]
struct LegacyTimelineTurn {
    prompt: String,
    assistant_stream: String,
    running: bool,
}

#[derive(Debug, Deserialize)]
struct LegacyTimelineSession {
    session_id: String,
    pid: u64,
    workload: String,
    turns: Vec<LegacyTimelineTurn>,
    error: Option<String>,
    #[serde(default)]
    system_events: Vec<(String, String)>,
}

impl StorageService {
    pub(crate) fn import_legacy_timelines_once(
        &mut self,
        timeline_dir: &Path,
    ) -> Result<LegacyTimelineImportReport, StorageError> {
        if self.legacy_import_already_completed()? {
            return Ok(LegacyTimelineImportReport::default());
        }

        let mut report = LegacyTimelineImportReport::default();
        if let Ok(entries) = fs::read_dir(timeline_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
                    continue;
                };
                if !extension.eq_ignore_ascii_case("json") {
                    continue;
                }

                match self.import_single_legacy_timeline(&path) {
                    Ok(file_report) => {
                        report.imported_sessions += file_report.imported_sessions;
                        report.imported_turns += file_report.imported_turns;
                        report.imported_messages += file_report.imported_messages;
                    }
                    Err(err) => {
                        tracing::warn!(
                            path = %path.display(),
                            %err,
                            "STORAGE: skipping malformed legacy timeline file"
                        );
                    }
                }
            }
        }

        let now = current_timestamp_ms();
        upsert_kernel_meta(
            &self.connection,
            LEGACY_IMPORT_META_KEY,
            &now.to_string(),
            now,
        )?;

        Ok(report)
    }

    pub(crate) fn start_session_turn(
        &mut self,
        session_id: &str,
        pid: u64,
        workload: &str,
        source: &str,
        prompt: &str,
        prompt_kind: &str,
    ) -> Result<i64, StorageError> {
        let started_at_ms = current_timestamp_ms();
        let transaction = self.connection.transaction()?;
        let turn_index = next_turn_index(&transaction, session_id)?;
        let run_id = active_run_id_for_session_pid(&transaction, session_id, pid)?.ok_or_else(|| {
            StorageError::MissingProcessRun {
                session_id: session_id.to_string(),
                pid,
            }
        })?;

        transaction.execute(
            r#"
            INSERT INTO session_turns (
                session_id,
                run_id,
                pid,
                turn_index,
                workload,
                source,
                status,
                started_at_ms,
                updated_at_ms,
                completed_at_ms,
                finish_reason
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running', ?7, ?7, NULL, NULL)
            "#,
            params![
                session_id,
                run_id,
                pid,
                turn_index,
                workload,
                source,
                started_at_ms
            ],
        )?;
        let turn_id = transaction.last_insert_rowid();
        insert_message(
            &transaction,
            session_id,
            turn_id,
            pid,
            1,
            "user",
            prompt_kind,
            prompt,
            started_at_ms,
        )?;
        transaction.execute(
            "UPDATE sessions SET updated_at_ms = ?2 WHERE session_id = ?1",
            params![session_id, started_at_ms],
        )?;
        transaction.commit()?;

        Ok(turn_id)
    }

    pub(crate) fn resume_turn(&mut self, turn_id: i64) -> Result<(), StorageError> {
        let updated_at_ms = current_timestamp_ms();
        let transaction = self.connection.transaction()?;
        let (session_id, _) = turn_identity(&transaction, turn_id)?
            .ok_or(StorageError::MissingTurn { turn_id })?;

        transaction.execute(
            r#"
            UPDATE session_turns
            SET status = 'running',
                updated_at_ms = ?2,
                completed_at_ms = NULL,
                finish_reason = NULL
            WHERE turn_id = ?1
            "#,
            params![turn_id, updated_at_ms],
        )?;
        transaction.execute(
            "UPDATE sessions SET updated_at_ms = ?2 WHERE session_id = ?1",
            params![session_id, updated_at_ms],
        )?;
        transaction.commit()?;

        Ok(())
    }

    pub(crate) fn append_assistant_message(
        &mut self,
        turn_id: i64,
        text: &str,
    ) -> Result<(), StorageError> {
        if text.is_empty() {
            return Ok(());
        }

        let created_at_ms = current_timestamp_ms();
        let transaction = self.connection.transaction()?;
        let (session_id, pid) = turn_identity(&transaction, turn_id)?
            .ok_or(StorageError::MissingTurn { turn_id })?;

        if let Some(message_id) = assistant_message_id_for_turn(&transaction, turn_id)? {
            transaction.execute(
                r#"
                UPDATE session_messages
                SET content = content || ?2
                WHERE message_id = ?1
                "#,
                params![message_id, text],
            )?;
        } else {
            let ordinal = next_message_ordinal(&transaction, turn_id)?;
            insert_message(
                &transaction,
                &session_id,
                turn_id,
                pid,
                ordinal,
                "assistant",
                "message",
                text,
                created_at_ms,
            )?;
        }
        transaction.execute(
            "UPDATE session_turns SET updated_at_ms = ?2 WHERE turn_id = ?1",
            params![turn_id, created_at_ms],
        )?;
        transaction.execute(
            "UPDATE sessions SET updated_at_ms = ?2 WHERE session_id = ?1",
            params![session_id, created_at_ms],
        )?;
        transaction.commit()?;

        Ok(())
    }

    pub(crate) fn finish_turn(
        &mut self,
        turn_id: i64,
        status: &str,
        finish_reason: &str,
        marker_text: Option<&str>,
    ) -> Result<(), StorageError> {
        let ended_at_ms = current_timestamp_ms();
        let transaction = self.connection.transaction()?;
        let (session_id, pid) = turn_identity(&transaction, turn_id)?
            .ok_or(StorageError::MissingTurn { turn_id })?;
        let (current_status, current_finish_reason): (String, Option<String>) = transaction
            .query_row(
                "SELECT status, finish_reason FROM session_turns WHERE turn_id = ?1",
                params![turn_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
        let preserve_terminal_status =
            matches!(current_status.as_str(), "terminated" | "killed" | "errored")
                && finish_reason == "completed";
        let persisted_status = if preserve_terminal_status {
            current_status.as_str()
        } else {
            status
        };
        let persisted_finish_reason = if preserve_terminal_status {
            current_finish_reason.as_deref().unwrap_or(finish_reason)
        } else {
            finish_reason
        };

        transaction.execute(
            r#"
            UPDATE session_turns
            SET status = ?2,
                updated_at_ms = ?3,
                completed_at_ms = COALESCE(completed_at_ms, ?3),
                finish_reason = ?4
            WHERE turn_id = ?1
            "#,
            params![
                turn_id,
                persisted_status,
                ended_at_ms,
                persisted_finish_reason
            ],
        )?;

        if let Some(marker_text) = marker_text.filter(|text| !text.trim().is_empty()) {
            let ordinal = next_message_ordinal(&transaction, turn_id)?;
            insert_message(
                &transaction,
                &session_id,
                turn_id,
                pid,
                ordinal,
                "system",
                "marker",
                marker_text,
                ended_at_ms,
            )?;
        }

        transaction.execute(
            "UPDATE sessions SET updated_at_ms = ?2 WHERE session_id = ?1",
            params![session_id, ended_at_ms],
        )?;
        transaction.commit()?;

        Ok(())
    }

    pub(crate) fn error_turn(
        &mut self,
        turn_id: i64,
        message: &str,
    ) -> Result<(), StorageError> {
        let ended_at_ms = current_timestamp_ms();
        let transaction = self.connection.transaction()?;
        let (session_id, pid) = turn_identity(&transaction, turn_id)?
            .ok_or(StorageError::MissingTurn { turn_id })?;

        transaction.execute(
            r#"
            UPDATE session_turns
            SET status = 'errored',
                updated_at_ms = ?2,
                completed_at_ms = COALESCE(completed_at_ms, ?2),
                finish_reason = 'worker_error'
            WHERE turn_id = ?1
            "#,
            params![turn_id, ended_at_ms],
        )?;
        let ordinal = next_message_ordinal(&transaction, turn_id)?;
        insert_message(
            &transaction,
            &session_id,
            turn_id,
            pid,
            ordinal,
            "system",
            "error",
            message,
            ended_at_ms,
        )?;
        transaction.execute(
            "UPDATE sessions SET updated_at_ms = ?2 WHERE session_id = ?1",
            params![session_id, ended_at_ms],
        )?;
        transaction.commit()?;

        Ok(())
    }

    fn legacy_import_already_completed(&self) -> Result<bool, StorageError> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM kernel_meta WHERE key = ?1",
                params![LEGACY_IMPORT_META_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .is_some())
    }

    fn import_single_legacy_timeline(
        &mut self,
        path: &Path,
    ) -> Result<LegacyTimelineImportReport, Box<dyn std::error::Error + Send + Sync>> {
        let payload = fs::read(path)?;
        let legacy = serde_json::from_slice::<LegacyTimelineSession>(&payload)?;
        let imported_at_ms = file_timestamp_ms(path).unwrap_or_else(current_timestamp_ms);
        let mut report = LegacyTimelineImportReport::default();

        let transaction = self.connection.transaction()?;
        ensure_session_exists(&transaction, &legacy.session_id, &legacy, imported_at_ms)?;

        let existing_turns: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM session_turns WHERE session_id = ?1",
            params![legacy.session_id],
            |row| row.get(0),
        )?;
        if existing_turns > 0 {
            transaction.commit()?;
            return Ok(report);
        }

        if !legacy.turns.is_empty() {
            report.imported_sessions = 1;
        }

        let run_id = ensure_legacy_process_run(
            &transaction,
            &legacy.session_id,
            legacy.pid,
            imported_at_ms,
        )?;

        for (index, turn) in legacy.turns.iter().enumerate() {
            let turn_index = (index as i64) + 1;
            let turn_started_at_ms = imported_at_ms + (index as i64);
            transaction.execute(
                r#"
                INSERT INTO session_turns (
                    session_id,
                    run_id,
                    pid,
                    turn_index,
                    workload,
                    source,
                    status,
                    started_at_ms,
                    updated_at_ms,
                    completed_at_ms,
                    finish_reason
                ) VALUES (?1, ?2, ?3, ?4, ?5, 'legacy_import', ?6, ?7, ?7, ?8, ?9)
                "#,
                params![
                    legacy.session_id,
                    run_id,
                    legacy.pid,
                    turn_index,
                    legacy.workload,
                    if turn.running { "running" } else { "completed" },
                    turn_started_at_ms,
                    if turn.running {
                        Option::<i64>::None
                    } else {
                        Some(turn_started_at_ms)
                    },
                    if turn.running {
                        Option::<String>::None
                    } else {
                        Some("legacy_import".to_string())
                    },
                ],
            )?;
            let turn_id = transaction.last_insert_rowid();
            report.imported_turns += 1;

            insert_message(
                &transaction,
                &legacy.session_id,
                turn_id,
                legacy.pid,
                1,
                "user",
                if index == 0 { "prompt" } else { "input" },
                &turn.prompt,
                turn_started_at_ms,
            )?;
            report.imported_messages += 1;

            if !turn.assistant_stream.trim().is_empty() {
                insert_message(
                    &transaction,
                    &legacy.session_id,
                    turn_id,
                    legacy.pid,
                    2,
                    "assistant",
                    "message",
                    &turn.assistant_stream,
                    turn_started_at_ms,
                )?;
                report.imported_messages += 1;
            }
        }

        if let Some(last_turn_id) = latest_turn_id_for_session(&transaction, &legacy.session_id)? {
            let mut next_ordinal = next_message_ordinal(&transaction, last_turn_id)?;
            for (text, status) in &legacy.system_events {
                insert_message(
                    &transaction,
                    &legacy.session_id,
                    last_turn_id,
                    legacy.pid,
                    next_ordinal,
                    "system",
                    if status == "error" { "error" } else { "marker" },
                    text,
                    imported_at_ms,
                )?;
                next_ordinal += 1;
                report.imported_messages += 1;
            }

            if let Some(error) = legacy.error.as_ref() {
                transaction.execute(
                    r#"
                    UPDATE session_turns
                    SET status = 'errored',
                        updated_at_ms = ?2,
                        completed_at_ms = COALESCE(completed_at_ms, ?2),
                        finish_reason = 'legacy_error'
                    WHERE turn_id = ?1
                    "#,
                    params![last_turn_id, imported_at_ms],
                )?;
                insert_message(
                    &transaction,
                    &legacy.session_id,
                    last_turn_id,
                    legacy.pid,
                    next_ordinal,
                    "system",
                    "error",
                    error,
                    imported_at_ms,
                )?;
                report.imported_messages += 1;
            }
        }

        transaction.execute(
            "UPDATE sessions SET updated_at_ms = ?2 WHERE session_id = ?1",
            params![legacy.session_id, imported_at_ms],
        )?;
        transaction.commit()?;

        Ok(report)
    }

    pub(crate) fn load_replay_messages_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<StoredReplayMessage>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT role, kind, content
            FROM session_messages
            WHERE session_id = ?1
            ORDER BY message_id ASC
            "#,
        )?;
        let rows = statement.query_map(params![session_id], |row| {
            Ok(StoredReplayMessage {
                role: row.get(0)?,
                kind: row.get(1)?,
                content: row.get(2)?,
            })
        })?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }
        Ok(messages)
    }

    pub(crate) fn latest_workload_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<String>, StorageError> {
        use rusqlite::OptionalExtension;

        self.connection
            .query_row(
                r#"
                SELECT workload
                FROM session_turns
                WHERE session_id = ?1
                ORDER BY turn_index DESC, turn_id DESC
                LIMIT 1
                "#,
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::from)
    }
}

fn next_turn_index(
    transaction: &Transaction<'_>,
    session_id: &str,
) -> Result<i64, rusqlite::Error> {
    transaction.query_row(
        "SELECT COALESCE(MAX(turn_index), 0) + 1 FROM session_turns WHERE session_id = ?1",
        params![session_id],
        |row| row.get(0),
    )
}

fn active_run_id_for_session_pid(
    transaction: &Transaction<'_>,
    session_id: &str,
    pid: u64,
) -> Result<Option<i64>, rusqlite::Error> {
    transaction
        .query_row(
            r#"
            SELECT run_id
            FROM process_runs
            WHERE session_id = ?1
              AND pid = ?2
              AND ended_at_ms IS NULL
            ORDER BY run_id DESC
            LIMIT 1
            "#,
            params![session_id, pid],
            |row| row.get(0),
        )
        .optional()
}

fn latest_turn_id_for_session(
    transaction: &Transaction<'_>,
    session_id: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    transaction
        .query_row(
            r#"
            SELECT turn_id
            FROM session_turns
            WHERE session_id = ?1
            ORDER BY turn_index DESC, turn_id DESC
            LIMIT 1
            "#,
            params![session_id],
            |row| row.get(0),
        )
        .optional()
}

fn turn_identity(
    transaction: &Transaction<'_>,
    turn_id: i64,
) -> Result<Option<(String, u64)>, rusqlite::Error> {
    transaction
        .query_row(
            "SELECT session_id, pid FROM session_turns WHERE turn_id = ?1",
            params![turn_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
}

fn assistant_message_id_for_turn(
    transaction: &Transaction<'_>,
    turn_id: i64,
) -> Result<Option<i64>, rusqlite::Error> {
    transaction
        .query_row(
            r#"
            SELECT message_id
            FROM session_messages
            WHERE turn_id = ?1
              AND role = 'assistant'
            ORDER BY ordinal ASC, message_id ASC
            LIMIT 1
            "#,
            params![turn_id],
            |row| row.get(0),
        )
        .optional()
}

fn latest_kernel_boot_id(transaction: &Transaction<'_>) -> Result<Option<i64>, rusqlite::Error> {
    transaction
        .query_row(
            "SELECT boot_id FROM kernel_boots ORDER BY boot_id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
}

fn ensure_legacy_process_run(
    transaction: &Transaction<'_>,
    session_id: &str,
    pid: u64,
    imported_at_ms: i64,
) -> Result<i64, StorageError> {
    if let Some(existing) = transaction
        .query_row(
            r#"
            SELECT run_id
            FROM process_runs
            WHERE session_id = ?1 AND pid = ?2
            ORDER BY run_id DESC
            LIMIT 1
            "#,
            params![session_id, pid],
            |row| row.get(0),
        )
        .optional()?
    {
        return Ok(existing);
    }

    let boot_id = latest_kernel_boot_id(transaction)?.ok_or(StorageError::MissingKernelBoot)?;
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
        ) VALUES (?1, ?2, ?3, NULL, 'legacy_import', ?4, ?4)
        "#,
        params![session_id, boot_id, pid, imported_at_ms],
    )?;
    Ok(transaction.last_insert_rowid())
}

pub(super) fn next_message_ordinal(
    transaction: &Transaction<'_>,
    turn_id: i64,
) -> Result<i64, rusqlite::Error> {
    transaction.query_row(
        "SELECT COALESCE(MAX(ordinal), 0) + 1 FROM session_messages WHERE turn_id = ?1",
        params![turn_id],
        |row| row.get(0),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn insert_message(
    transaction: &Transaction<'_>,
    session_id: &str,
    turn_id: i64,
    pid: u64,
    ordinal: i64,
    role: &str,
    kind: &str,
    content: &str,
    created_at_ms: i64,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        r#"
        INSERT INTO session_messages (
            session_id,
            turn_id,
            pid,
            ordinal,
            role,
            kind,
            content,
            created_at_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        params![
            session_id,
            turn_id,
            pid,
            ordinal,
            role,
            kind,
            content,
            created_at_ms
        ],
    )?;
    Ok(())
}

fn ensure_session_exists(
    transaction: &Transaction<'_>,
    session_id: &str,
    legacy: &LegacyTimelineSession,
    imported_at_ms: i64,
) -> Result<(), rusqlite::Error> {
    let title = legacy
        .turns
        .first()
        .map(|turn| turn.prompt.lines().next().unwrap_or_default().trim())
        .filter(|title| !title.is_empty())
        .unwrap_or(session_id);
    transaction.execute(
        r#"
        INSERT INTO sessions (
            session_id,
            title,
            status,
            active_pid,
            created_at_ms,
            updated_at_ms
        ) VALUES (?1, ?2, 'idle', NULL, ?3, ?3)
        ON CONFLICT(session_id) DO NOTHING
        "#,
        params![session_id, title, imported_at_ms],
    )?;
    Ok(())
}

fn file_timestamp_ms(path: &Path) -> Option<i64> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as i64)
}

#[cfg(test)]
#[path = "timeline_tests.rs"]
mod tests;
