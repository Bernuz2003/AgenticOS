use rusqlite::params;
#[cfg(test)]
use rusqlite::OptionalExtension;

use super::artifacts::{
    derive_result_artifact_text, preview_text, primary_artifact_id, StoredWorkflowArtifact,
    StoredWorkflowArtifactInput, WorkflowArtifactInputRef,
};
use crate::storage::{StorageError, StorageService};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredWorkflowTaskAttempt {
    pub orchestration_id: u64,
    pub task_id: String,
    pub attempt: u32,
    pub status: String,
    pub session_id: Option<String>,
    pub pid: Option<u64>,
    pub error: Option<String>,
    pub output_preview: String,
    pub output_chars: usize,
    pub truncated: bool,
    pub started_at_ms: i64,
    pub updated_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub primary_artifact_id: Option<String>,
    pub termination_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredWorkflowIo {
    pub attempts: Vec<StoredWorkflowTaskAttempt>,
    pub artifacts: Vec<StoredWorkflowArtifact>,
    pub inputs: Vec<StoredWorkflowArtifactInput>,
}

impl StorageService {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin_workflow_task_attempt(
        &mut self,
        orchestration_id: u64,
        task_id: &str,
        attempt: u32,
        session_id: Option<&str>,
        pid: Option<u64>,
        started_at_ms: i64,
        input_artifacts: &[WorkflowArtifactInputRef],
    ) -> Result<(), StorageError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            r#"
            INSERT INTO workflow_task_attempts (
                orchestration_id,
                task_id,
                attempt,
                status,
                session_id,
                pid,
                error,
                output_preview,
                output_chars,
                truncated,
                started_at_ms,
                updated_at_ms,
                completed_at_ms,
                primary_artifact_id,
                termination_reason
            ) VALUES (?1, ?2, ?3, 'running', ?4, ?5, NULL, '', 0, 0, ?6, ?6, NULL, NULL, NULL)
            ON CONFLICT(orchestration_id, task_id, attempt) DO UPDATE SET
                status = excluded.status,
                session_id = excluded.session_id,
                pid = excluded.pid,
                error = NULL,
                updated_at_ms = excluded.updated_at_ms,
                termination_reason = NULL
            "#,
            params![
                orchestration_id,
                task_id,
                attempt,
                session_id,
                pid,
                started_at_ms,
            ],
        )?;
        transaction.execute(
            r#"
            DELETE FROM workflow_task_artifact_inputs
            WHERE orchestration_id = ?1 AND consumer_task_id = ?2 AND consumer_attempt = ?3
            "#,
            params![orchestration_id, task_id, attempt],
        )?;
        for input in input_artifacts {
            transaction.execute(
                r#"
                INSERT INTO workflow_task_artifact_inputs (
                    orchestration_id,
                    consumer_task_id,
                    consumer_attempt,
                    artifact_id,
                    producer_task_id,
                    producer_attempt
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    orchestration_id,
                    task_id,
                    attempt,
                    input.artifact_id,
                    input.producer_task_id,
                    input.producer_attempt,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn record_workflow_task_spawn_failure(
        &mut self,
        orchestration_id: u64,
        task_id: &str,
        attempt: u32,
        error: &str,
        recorded_at_ms: i64,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            r#"
            INSERT INTO workflow_task_attempts (
                orchestration_id,
                task_id,
                attempt,
                status,
                session_id,
                pid,
                error,
                output_preview,
                output_chars,
                truncated,
                started_at_ms,
                updated_at_ms,
                completed_at_ms,
                primary_artifact_id,
                termination_reason
            ) VALUES (?1, ?2, ?3, 'failed', NULL, NULL, ?4, '', 0, 0, ?5, ?5, ?5, NULL, 'spawn_failed')
            ON CONFLICT(orchestration_id, task_id, attempt) DO UPDATE SET
                status = 'failed',
                pid = NULL,
                error = excluded.error,
                updated_at_ms = excluded.updated_at_ms,
                completed_at_ms = excluded.completed_at_ms,
                termination_reason = excluded.termination_reason
            "#,
            params![orchestration_id, task_id, attempt, error, recorded_at_ms],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn finalize_workflow_task_attempt(
        &mut self,
        orchestration_id: u64,
        task_id: &str,
        attempt: u32,
        status: &str,
        error: Option<&str>,
        termination_reason: Option<&str>,
        output_text: &str,
        truncated: bool,
        completed_at_ms: i64,
    ) -> Result<Option<StoredWorkflowArtifact>, StorageError> {
        let result_text = derive_result_artifact_text(output_text);
        let preview = preview_text(&result_text);
        let output_chars = output_text.len() as i64;
        let artifact = if status == "completed" || !result_text.is_empty() {
            Some(StoredWorkflowArtifact {
                artifact_id: primary_artifact_id(orchestration_id, task_id, attempt),
                orchestration_id,
                producer_task_id: task_id.to_string(),
                producer_attempt: attempt,
                kind: if status == "completed" {
                    "task_result".to_string()
                } else {
                    "task_result_partial".to_string()
                },
                label: format!("{task_id} result"),
                mime_type: "text/markdown".to_string(),
                content_text: result_text.clone(),
                preview: preview.clone(),
                bytes: result_text.len(),
                created_at_ms: completed_at_ms,
            })
        } else {
            None
        };

        let transaction = self.connection.transaction()?;
        if let Some(artifact) = artifact.as_ref() {
            transaction.execute(
                r#"
                INSERT INTO workflow_artifacts (
                    artifact_id,
                    orchestration_id,
                    producer_task_id,
                    producer_attempt,
                    kind,
                    label,
                    mime_type,
                    content_text,
                    preview,
                    bytes,
                    created_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                ON CONFLICT(artifact_id) DO UPDATE SET
                    kind = excluded.kind,
                    label = excluded.label,
                    mime_type = excluded.mime_type,
                    content_text = excluded.content_text,
                    preview = excluded.preview,
                    bytes = excluded.bytes,
                    created_at_ms = excluded.created_at_ms
                "#,
                params![
                    artifact.artifact_id,
                    artifact.orchestration_id,
                    artifact.producer_task_id,
                    artifact.producer_attempt,
                    artifact.kind,
                    artifact.label,
                    artifact.mime_type,
                    artifact.content_text,
                    artifact.preview,
                    artifact.bytes as i64,
                    artifact.created_at_ms,
                ],
            )?;
        }

        transaction.execute(
            r#"
            INSERT INTO workflow_task_attempts (
                orchestration_id,
                task_id,
                attempt,
                status,
                session_id,
                pid,
                error,
                output_preview,
                output_chars,
                truncated,
                started_at_ms,
                updated_at_ms,
                completed_at_ms,
                primary_artifact_id,
                termination_reason
            ) VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, ?6, ?7, ?8, ?9, ?9, ?9, ?10, ?11)
            ON CONFLICT(orchestration_id, task_id, attempt) DO UPDATE SET
                status = excluded.status,
                error = excluded.error,
                output_preview = excluded.output_preview,
                output_chars = excluded.output_chars,
                truncated = excluded.truncated,
                updated_at_ms = excluded.updated_at_ms,
                completed_at_ms = excluded.completed_at_ms,
                primary_artifact_id = excluded.primary_artifact_id,
                termination_reason = excluded.termination_reason
            "#,
            params![
                orchestration_id,
                task_id,
                attempt,
                status,
                error,
                preview,
                output_chars,
                truncated,
                completed_at_ms,
                artifact.as_ref().map(|entry| entry.artifact_id.as_str()),
                termination_reason,
            ],
        )?;
        transaction.commit()?;

        Ok(artifact)
    }

    pub(crate) fn load_workflow_io(
        &self,
        orchestration_id: u64,
    ) -> Result<StoredWorkflowIo, StorageError> {
        Ok(StoredWorkflowIo {
            attempts: self.load_workflow_task_attempts(orchestration_id)?,
            artifacts: self.load_workflow_artifacts(orchestration_id)?,
            inputs: self.load_workflow_artifact_inputs(orchestration_id)?,
        })
    }

    pub(crate) fn delete_workflow_io(&mut self, orchestration_id: u64) -> Result<(), StorageError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM workflow_task_artifact_inputs WHERE orchestration_id = ?1",
            params![orchestration_id],
        )?;
        transaction.execute(
            "DELETE FROM workflow_artifacts WHERE orchestration_id = ?1",
            params![orchestration_id],
        )?;
        transaction.execute(
            "DELETE FROM workflow_task_attempts WHERE orchestration_id = ?1",
            params![orchestration_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn load_workflow_task_attempts(
        &self,
        orchestration_id: u64,
    ) -> Result<Vec<StoredWorkflowTaskAttempt>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                orchestration_id,
                task_id,
                attempt,
                status,
                session_id,
                pid,
                error,
                output_preview,
                output_chars,
                truncated,
                started_at_ms,
                updated_at_ms,
                completed_at_ms,
                primary_artifact_id,
                termination_reason
            FROM workflow_task_attempts
            WHERE orchestration_id = ?1
            ORDER BY task_id ASC, attempt DESC
            "#,
        )?;
        let rows = statement.query_map(params![orchestration_id], |row| {
            Ok(StoredWorkflowTaskAttempt {
                orchestration_id: row.get(0)?,
                task_id: row.get(1)?,
                attempt: row.get(2)?,
                status: row.get(3)?,
                session_id: row.get(4)?,
                pid: row.get(5)?,
                error: row.get(6)?,
                output_preview: row.get(7)?,
                output_chars: row.get::<_, i64>(8)? as usize,
                truncated: row.get::<_, bool>(9)?,
                started_at_ms: row.get(10)?,
                updated_at_ms: row.get(11)?,
                completed_at_ms: row.get(12)?,
                primary_artifact_id: row.get(13)?,
                termination_reason: row.get(14)?,
            })
        })?;
        collect_rows(rows)
    }

    #[cfg(test)]
    pub(crate) fn workflow_task_attempt_by_key(
        &self,
        orchestration_id: u64,
        task_id: &str,
        attempt: u32,
    ) -> Result<Option<StoredWorkflowTaskAttempt>, StorageError> {
        Ok(self
            .connection
            .query_row(
                r#"
                SELECT
                    orchestration_id,
                    task_id,
                    attempt,
                    status,
                    session_id,
                    pid,
                    error,
                    output_preview,
                    output_chars,
                    truncated,
                        started_at_ms,
                        updated_at_ms,
                        completed_at_ms,
                        primary_artifact_id,
                        termination_reason
                    FROM workflow_task_attempts
                    WHERE orchestration_id = ?1 AND task_id = ?2 AND attempt = ?3
                    "#,
                params![orchestration_id, task_id, attempt],
                |row| {
                    Ok(StoredWorkflowTaskAttempt {
                        orchestration_id: row.get(0)?,
                        task_id: row.get(1)?,
                        attempt: row.get(2)?,
                        status: row.get(3)?,
                        session_id: row.get(4)?,
                        pid: row.get(5)?,
                        error: row.get(6)?,
                        output_preview: row.get(7)?,
                        output_chars: row.get::<_, i64>(8)? as usize,
                        truncated: row.get::<_, bool>(9)?,
                        started_at_ms: row.get(10)?,
                        updated_at_ms: row.get(11)?,
                        completed_at_ms: row.get(12)?,
                        primary_artifact_id: row.get(13)?,
                        termination_reason: row.get(14)?,
                    })
                },
            )
            .optional()?)
    }
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>, StorageError> {
    let mut values = Vec::new();
    for row in rows {
        values.push(row?);
    }
    Ok(values)
}

#[cfg(test)]
#[path = "tests/orchestration_state.rs"]
mod tests;
