use super::{TaskInputArtifact, TaskNodeDef};

const TRUNCATION_MARKER: &str = "\n[TRUNCATED]\n";
const WORKFLOW_TASK_CONTRACT: &str = "\
You are executing one task inside a structured workflow pipeline.
- Treat upstream artifacts as authoritative inputs from previous tasks.
- Use tools only when necessary to complete the task.
- Do not include planning chatter, repeated tool intentions, or transcript-style narration in the durable result.
- Your final answer must contain a section titled [Result Artifact] with the clean result that downstream tasks should consume.
- Keep extra notes short and place them after the result artifact.";

pub(crate) fn build_task_prompt(
    task: &TaskNodeDef,
    input_artifacts: &[TaskInputArtifact],
) -> String {
    let task_prompt = match task
        .role
        .as_deref()
        .map(str::trim)
        .filter(|role| !role.is_empty())
    {
        Some(role) => format!(
            "[Task role]\n{}\n\n[Task instructions]\n{}",
            role, task.prompt
        ),
        None => format!("[Task instructions]\n{}", task.prompt),
    };
    let mut sections = vec![format!(
        "[Workflow task contract]\n{}",
        WORKFLOW_TASK_CONTRACT
    )];
    let mut artifact_sections = Vec::new();
    for artifact in input_artifacts {
        if artifact.content_text.trim().is_empty() {
            continue;
        }

        artifact_sections.push(format!(
            "[Result artifact from task \"{}\" attempt {} | id={} | type={}]\n{}",
            artifact.producer_task_id,
            artifact.producer_attempt,
            artifact.artifact_id,
            artifact.mime_type,
            artifact.content_text
        ));
    }

    if !artifact_sections.is_empty() {
        sections.push(format!(
            "[Upstream result artifacts]\n{}",
            artifact_sections.join("\n\n")
        ));
    }

    sections.push(task_prompt);
    sections.join("\n\n")
}

pub(crate) fn append_with_cap(
    target: &mut String,
    incoming: &str,
    cap: usize,
    truncations: &mut usize,
) {
    if cap == 0 {
        return;
    }

    if target.len() >= cap {
        if !target.contains(TRUNCATION_MARKER) {
            ensure_truncation_marker(target, cap);
            *truncations += 1;
        }
        return;
    }

    let remaining = cap.saturating_sub(target.len());
    if incoming.len() <= remaining {
        target.push_str(incoming);
        return;
    }

    let keep =
        truncate_to_char_boundary(incoming, remaining.saturating_sub(TRUNCATION_MARKER.len()));
    if keep > 0 {
        target.push_str(&incoming[..keep]);
    }
    ensure_truncation_marker(target, cap);
    *truncations += 1;
}

fn ensure_truncation_marker(target: &mut String, cap: usize) {
    if target.contains(TRUNCATION_MARKER) {
        return;
    }

    if cap <= TRUNCATION_MARKER.len() {
        target.clear();
        target.push_str(&TRUNCATION_MARKER[..cap]);
        return;
    }

    while target.len() + TRUNCATION_MARKER.len() > cap {
        target.pop();
        while !target.is_char_boundary(target.len()) {
            target.pop();
        }
    }
    target.push_str(TRUNCATION_MARKER);
}

fn truncate_to_char_boundary(text: &str, max_len: usize) -> usize {
    if max_len >= text.len() {
        return text.len();
    }
    let mut idx = max_len;
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}
