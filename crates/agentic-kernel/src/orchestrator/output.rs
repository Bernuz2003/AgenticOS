use std::collections::HashMap;

use super::TaskNodeDef;

const TRUNCATION_MARKER: &str = "\n[TRUNCATED]\n";

pub(crate) fn build_task_prompt(task: &TaskNodeDef, outputs: &HashMap<String, String>) -> String {
    let mut context_parts = Vec::new();
    for dep in &task.deps {
        if let Some(output) = outputs.get(dep) {
            if !output.is_empty() {
                context_parts.push(format!("[Output from task \"{}\"]:\n{}", dep, output));
            }
        }
    }

    if context_parts.is_empty() {
        task.prompt.clone()
    } else {
        format!(
            "{}\n\n[Your task]:\n{}",
            context_parts.join("\n\n"),
            task.prompt,
        )
    }
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
