use rusqlite::params;
#[cfg(test)]
use rusqlite::OptionalExtension;

use super::service::{StorageError, StorageService};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkflowArtifactInputRef {
    pub artifact_id: String,
    pub producer_task_id: String,
    pub producer_attempt: u32,
}

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
pub(crate) struct StoredWorkflowArtifact {
    pub artifact_id: String,
    pub orchestration_id: u64,
    pub producer_task_id: String,
    pub producer_attempt: u32,
    pub kind: String,
    pub label: String,
    pub mime_type: String,
    pub content_text: String,
    pub preview: String,
    pub bytes: usize,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredWorkflowArtifactInput {
    pub orchestration_id: u64,
    pub consumer_task_id: String,
    pub consumer_attempt: u32,
    pub artifact_id: String,
    pub producer_task_id: String,
    pub producer_attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredWorkflowIo {
    pub attempts: Vec<StoredWorkflowTaskAttempt>,
    pub artifacts: Vec<StoredWorkflowArtifact>,
    pub inputs: Vec<StoredWorkflowArtifactInput>,
}

impl StorageService {
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

    fn load_workflow_artifacts(
        &self,
        orchestration_id: u64,
    ) -> Result<Vec<StoredWorkflowArtifact>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT
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
            FROM workflow_artifacts
            WHERE orchestration_id = ?1
            ORDER BY created_at_ms DESC, artifact_id DESC
            "#,
        )?;
        let rows = statement.query_map(params![orchestration_id], |row| {
            Ok(StoredWorkflowArtifact {
                artifact_id: row.get(0)?,
                orchestration_id: row.get(1)?,
                producer_task_id: row.get(2)?,
                producer_attempt: row.get(3)?,
                kind: row.get(4)?,
                label: row.get(5)?,
                mime_type: row.get(6)?,
                content_text: row.get(7)?,
                preview: row.get(8)?,
                bytes: row.get::<_, i64>(9)? as usize,
                created_at_ms: row.get(10)?,
            })
        })?;
        collect_rows(rows)
    }

    fn load_workflow_artifact_inputs(
        &self,
        orchestration_id: u64,
    ) -> Result<Vec<StoredWorkflowArtifactInput>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                orchestration_id,
                consumer_task_id,
                consumer_attempt,
                artifact_id,
                producer_task_id,
                producer_attempt
            FROM workflow_task_artifact_inputs
            WHERE orchestration_id = ?1
            ORDER BY consumer_task_id ASC, consumer_attempt DESC, producer_task_id ASC
            "#,
        )?;
        let rows = statement.query_map(params![orchestration_id], |row| {
            Ok(StoredWorkflowArtifactInput {
                orchestration_id: row.get(0)?,
                consumer_task_id: row.get(1)?,
                consumer_attempt: row.get(2)?,
                artifact_id: row.get(3)?,
                producer_task_id: row.get(4)?,
                producer_attempt: row.get(5)?,
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

pub(crate) fn primary_artifact_id(orchestration_id: u64, task_id: &str, attempt: u32) -> String {
    format!("orch:{orchestration_id}:task:{task_id}:attempt:{attempt}:result")
}

fn preview_text(content: &str) -> String {
    const MAX_CHARS: usize = 240;
    if content.chars().count() <= MAX_CHARS {
        return content.to_string();
    }

    let mut preview = content.chars().take(MAX_CHARS).collect::<String>();
    preview.push_str("...");
    preview
}

fn derive_result_artifact_text(raw_output: &str) -> String {
    let normalized = raw_output.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Some(explicit) = extract_result_artifact_block(trimmed) {
        return explicit;
    }

    let trailing_after_control = take_after_last_control_line(trimmed);
    let filtered_trailing = strip_non_result_lines(&trailing_after_control);
    if !filtered_trailing.is_empty() {
        return filtered_trailing;
    }

    let filtered_full = strip_non_result_lines(trimmed);
    if !filtered_full.is_empty() {
        return filtered_full;
    }

    trimmed.to_string()
}

fn extract_result_artifact_block(text: &str) -> Option<String> {
    let lines = text.lines().collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let lowered = trimmed.to_ascii_lowercase();
        let marker_match = matches!(
            lowered.as_str(),
            "[result artifact]"
                | "# result artifact"
                | "## result artifact"
                | "### result artifact"
        ) || lowered.starts_with("result artifact:")
            || lowered.starts_with("final result:")
            || lowered.starts_with("result:");
        if !marker_match {
            continue;
        }

        let mut collected = Vec::new();
        if let Some((_, inline)) = trimmed.split_once(':') {
            let inline = inline.trim();
            if !inline.is_empty() {
                collected.push(inline.to_string());
            }
        }
        for candidate in lines.iter().skip(index + 1) {
            let candidate_trimmed = candidate.trim();
            let lowered_candidate = candidate_trimmed.to_ascii_lowercase();
            if !collected.is_empty()
                && (looks_like_section_boundary(candidate_trimmed)
                    || matches!(
                        lowered_candidate.as_str(),
                        "[notes]" | "[metadata]" | "[transcript]" | "[tools]"
                    ))
            {
                break;
            }
            collected.push((*candidate).to_string());
        }

        let extracted = collapse_blank_lines(&collected.join("\n"));
        if !extracted.is_empty() {
            return Some(extracted);
        }
    }

    None
}

fn take_after_last_control_line(text: &str) -> String {
    let lines = text.lines().collect::<Vec<_>>();
    let Some(last_control_index) = lines.iter().rposition(|line| is_control_line(line.trim()))
    else {
        return text.to_string();
    };

    let trailing = lines
        .iter()
        .skip(last_control_index + 1)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    let trailing = trailing.trim();
    if trailing.is_empty() {
        text.to_string()
    } else {
        trailing.to_string()
    }
}

fn strip_non_result_lines(text: &str) -> String {
    let mut kept = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let lowered = trimmed.to_ascii_lowercase();
        if trimmed.is_empty() {
            kept.push(String::new());
            continue;
        }
        if is_control_line(trimmed)
            || matches!(
                lowered.as_str(),
                "<thinking>" | "</thinking>" | "<analysis>" | "</analysis>"
            )
        {
            continue;
        }
        kept.push(line.to_string());
    }
    collapse_blank_lines(&kept.join("\n"))
}

fn collapse_blank_lines(text: &str) -> String {
    let mut compacted = String::new();
    let mut previous_blank = false;
    for line in text.lines() {
        let is_blank = line.trim().is_empty();
        if is_blank && previous_blank {
            continue;
        }
        if !compacted.is_empty() {
            compacted.push('\n');
        }
        compacted.push_str(line.trim_end());
        previous_blank = is_blank;
    }
    compacted.trim().to_string()
}

fn is_control_line(line: &str) -> bool {
    line.starts_with("TOOL:")
        || line.starts_with("ACTION:")
        || line.starts_with("[TOOL")
        || line.starts_with("[ACTION")
}

fn looks_like_section_boundary(line: &str) -> bool {
    line.starts_with('[') || line.starts_with('#')
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
#[path = "workflow_artifacts_tests.rs"]
mod tests;
