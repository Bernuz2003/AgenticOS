use rusqlite::params;

use crate::storage::{StorageError, StorageService};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkflowArtifactInputRef {
    pub artifact_id: String,
    pub producer_task_id: String,
    pub producer_attempt: u32,
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

impl StorageService {
    pub(super) fn load_workflow_artifacts(
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

    pub(super) fn load_workflow_artifact_inputs(
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
}

pub(crate) fn primary_artifact_id(orchestration_id: u64, task_id: &str, attempt: u32) -> String {
    format!("orch:{orchestration_id}:task:{task_id}:attempt:{attempt}:result")
}

pub(super) fn preview_text(content: &str) -> String {
    const MAX_CHARS: usize = 240;
    if content.chars().count() <= MAX_CHARS {
        return content.to_string();
    }

    let mut preview = content.chars().take(MAX_CHARS).collect::<String>();
    preview.push_str("...");
    preview
}

pub(super) fn derive_result_artifact_text(raw_output: &str) -> String {
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
