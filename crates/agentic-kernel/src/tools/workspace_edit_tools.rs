use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use agentic_kernel_macros::agentic_tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::error::ToolError;
use super::invocation::ToolContext;
use super::path_guard::resolve_safe_path_for_context;
use super::workspace_tools::{
    ensure_non_empty_path, read_required_utf8_file, resolve_search_root,
    to_workspace_relative_string, MAX_TEXT_FILE_BYTES,
};

const DEFAULT_TREE_MAX_DEPTH: u64 = 4;
const MAX_TREE_DEPTH: u64 = 8;
const DEFAULT_TREE_MAX_ENTRIES: usize = 200;
const MAX_TREE_ENTRIES: usize = 500;
const DEFAULT_DIFF_MAX_CHANGES: usize = 100;
const MAX_DIFF_CHANGES: usize = 250;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AppendFileInput {
    path: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct AppendFileOutput {
    output: String,
    path: String,
    bytes_appended: usize,
    created: bool,
}

#[agentic_tool(
    name = "append_file",
    description = "Append UTF-8 text to a file inside the process-scoped workspace, creating the file and parent directories when needed.",
    input_example = serde_json::json!({"path": "reports/daily.log", "content": "job completed\n"}),
    capabilities = ["fs", "write", "append"],
    dangerous = true,
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn append_file(input: AppendFileInput, ctx: &ToolContext) -> Result<AppendFileOutput, ToolError> {
    ensure_non_empty_path("append_file", &input.path)?;
    let path = resolve_safe_path_for_context(&input.path, ctx)
        .map_err(|err| ToolError::ExecutionFailed("append_file".into(), err))?;
    let created = !path.exists();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ToolError::ExecutionFailed(
                "append_file".into(),
                format!("Failed to create parent dir: {err}"),
            )
        })?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| {
            ToolError::ExecutionFailed("append_file".into(), format!("Open failed: {err}"))
        })?;
    file.write_all(input.content.as_bytes()).map_err(|err| {
        ToolError::ExecutionFailed("append_file".into(), format!("Append failed: {err}"))
    })?;

    Ok(AppendFileOutput {
        output: format!(
            "Appended {} bytes to '{}'{}.",
            input.content.len(),
            input.path,
            if created { " (created)" } else { "" }
        ),
        path: input.path,
        bytes_appended: input.content.len(),
        created,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReplaceInFileInput {
    path: String,
    find: String,
    replace: String,
    replace_all: Option<bool>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct ReplaceInFileOutput {
    output: String,
    path: String,
    replacements: u64,
    bytes_written: usize,
}

#[agentic_tool(
    name = "replace_in_file",
    description = "Replace exact text inside a UTF-8 file in the process-scoped workspace.",
    input_example = serde_json::json!({"path": "docs/notes.md", "find": "TODO", "replace": "DONE", "replace_all": false}),
    capabilities = ["fs", "write", "replace"],
    dangerous = true,
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn replace_in_file(
    input: ReplaceInFileInput,
    ctx: &ToolContext,
) -> Result<ReplaceInFileOutput, ToolError> {
    ensure_non_empty_path("replace_in_file", &input.path)?;
    if input.find.is_empty() {
        return Err(ToolError::InvalidInput(
            "replace_in_file".into(),
            "field 'find' cannot be empty".into(),
        ));
    }

    let path = resolve_safe_path_for_context(&input.path, ctx)
        .map_err(|err| ToolError::ExecutionFailed("replace_in_file".into(), err))?;
    let content = read_required_utf8_file("replace_in_file", &path, MAX_TEXT_FILE_BYTES)?;
    let replacements = content.matches(&input.find).count() as u64;

    if replacements == 0 {
        return Err(ToolError::InvalidInput(
            "replace_in_file".into(),
            format!("'find' did not match any text in '{}'.", input.path),
        ));
    }

    let replace_all = input.replace_all.unwrap_or(false);
    if !replace_all && replacements > 1 {
        return Err(ToolError::InvalidInput(
            "replace_in_file".into(),
            format!(
                "'find' matched {} times in '{}'; set 'replace_all' to true or make the match more specific.",
                replacements, input.path
            ),
        ));
    }

    let updated = if replace_all {
        content.replace(&input.find, &input.replace)
    } else {
        content.replacen(&input.find, &input.replace, 1)
    };
    fs::write(&path, updated.as_bytes()).map_err(|err| {
        ToolError::ExecutionFailed("replace_in_file".into(), format!("Write failed: {err}"))
    })?;

    Ok(ReplaceInFileOutput {
        output: format!(
            "Replaced {} occurrence{} in '{}'.",
            if replace_all { replacements } else { 1 },
            if (replace_all && replacements == 1) || (!replace_all) {
                ""
            } else {
                "s"
            },
            input.path
        ),
        path: input.path,
        replacements: if replace_all { replacements } else { 1 },
        bytes_written: updated.len(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListTreeInput {
    path: Option<String>,
    max_depth: Option<u64>,
    max_entries: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct TreeEntry {
    path: String,
    entry_type: String,
    depth: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct ListTreeOutput {
    output: String,
    root: String,
    entries: Vec<TreeEntry>,
    truncated: bool,
}

#[agentic_tool(
    name = "list_tree",
    description = "Render a depth-limited file tree for a process-scoped workspace path.",
    input_example = serde_json::json!({"path": "crates/agentic-kernel/src/tools", "max_depth": 2, "max_entries": 50}),
    capabilities = ["fs", "tree", "list"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn list_tree(input: ListTreeInput, ctx: &ToolContext) -> Result<ListTreeOutput, ToolError> {
    let root = resolve_search_root("list_tree", input.path.as_deref(), ctx)?;
    let max_depth = input
        .max_depth
        .unwrap_or(DEFAULT_TREE_MAX_DEPTH)
        .min(MAX_TREE_DEPTH);
    let max_entries = normalize_limit(
        input.max_entries,
        DEFAULT_TREE_MAX_ENTRIES,
        MAX_TREE_ENTRIES,
    );
    let mut entries = Vec::new();
    let mut truncated = false;

    collect_tree_entries(
        "list_tree",
        &root.absolute,
        0,
        max_depth,
        max_entries,
        &mut entries,
        &mut truncated,
    )?;

    let output = if entries.is_empty() {
        format!("No entries found under '{}'.", root.display)
    } else {
        let mut lines = Vec::with_capacity(entries.len() + 1);
        lines.push(format!("Tree for '{}':", root.display));
        for entry in &entries {
            let indent = "  ".repeat(entry.depth as usize);
            let label = if entry.depth == 0 {
                entry.path.clone()
            } else {
                Path::new(&entry.path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(entry.path.as_str())
                    .to_string()
            };
            lines.push(format!("{}- {} [{}]", indent, label, entry.entry_type));
        }
        if truncated {
            lines.push("... (tree truncated)".to_string());
        }
        lines.join("\n")
    };

    Ok(ListTreeOutput {
        output,
        root: root.display,
        entries,
        truncated,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DiffFilesInput {
    left_path: String,
    right_path: String,
    max_changes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct DiffLine {
    line: u64,
    left: Option<String>,
    right: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct DiffFilesOutput {
    output: String,
    left_path: String,
    right_path: String,
    identical: bool,
    changes: Vec<DiffLine>,
    truncated: bool,
}

#[agentic_tool(
    name = "diff_files",
    description = "Compare two UTF-8 text files inside the process-scoped workspace.",
    input_example = serde_json::json!({"left_path": "before.txt", "right_path": "after.txt", "max_changes": 20}),
    capabilities = ["fs", "read", "diff"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn diff_files(input: DiffFilesInput, ctx: &ToolContext) -> Result<DiffFilesOutput, ToolError> {
    ensure_non_empty_path("diff_files", &input.left_path)?;
    ensure_non_empty_path("diff_files", &input.right_path)?;

    let left_path = resolve_safe_path_for_context(&input.left_path, ctx)
        .map_err(|err| ToolError::ExecutionFailed("diff_files".into(), err))?;
    let right_path = resolve_safe_path_for_context(&input.right_path, ctx)
        .map_err(|err| ToolError::ExecutionFailed("diff_files".into(), err))?;
    let left = read_required_utf8_file("diff_files", &left_path, MAX_TEXT_FILE_BYTES)?;
    let right = read_required_utf8_file("diff_files", &right_path, MAX_TEXT_FILE_BYTES)?;

    if left == right {
        return Ok(DiffFilesOutput {
            output: format!(
                "Files '{}' and '{}' are identical.",
                input.left_path, input.right_path
            ),
            left_path: input.left_path,
            right_path: input.right_path,
            identical: true,
            changes: Vec::new(),
            truncated: false,
        });
    }

    let max_changes = normalize_limit(
        input.max_changes,
        DEFAULT_DIFF_MAX_CHANGES,
        MAX_DIFF_CHANGES,
    );
    let left_lines: Vec<&str> = left.lines().collect();
    let right_lines: Vec<&str> = right.lines().collect();
    let total_lines = left_lines.len().max(right_lines.len());
    let mut changes = Vec::new();
    let mut truncated = false;

    for index in 0..total_lines {
        let left_line = left_lines.get(index).copied();
        let right_line = right_lines.get(index).copied();
        if left_line == right_line {
            continue;
        }

        changes.push(DiffLine {
            line: index as u64 + 1,
            left: left_line.map(ToString::to_string),
            right: right_line.map(ToString::to_string),
        });
        if changes.len() >= max_changes {
            truncated = true;
            break;
        }
    }

    let mut summary = Vec::with_capacity(changes.len() * 3 + 1);
    summary.push(format!(
        "Differences between '{}' and '{}':",
        input.left_path, input.right_path
    ));
    for change in &changes {
        summary.push(format!("Line {}:", change.line));
        summary.push(format!(
            "- {}",
            change.left.as_deref().unwrap_or("<missing>")
        ));
        summary.push(format!(
            "+ {}",
            change.right.as_deref().unwrap_or("<missing>")
        ));
    }
    if truncated {
        summary.push("... (diff truncated)".to_string());
    }

    Ok(DiffFilesOutput {
        output: summary.join("\n"),
        left_path: input.left_path,
        right_path: input.right_path,
        identical: false,
        changes,
        truncated,
    })
}

fn normalize_limit(raw: Option<u64>, default: usize, max: usize) -> usize {
    raw.unwrap_or(default as u64).clamp(1, max as u64) as usize
}

fn collect_tree_entries(
    tool_name: &str,
    path: &Path,
    depth: u64,
    max_depth: u64,
    max_entries: usize,
    entries: &mut Vec<TreeEntry>,
    truncated: &mut bool,
) -> Result<(), ToolError> {
    if entries.len() >= max_entries {
        *truncated = true;
        return Ok(());
    }

    let metadata = fs::symlink_metadata(path).map_err(|err| {
        ToolError::ExecutionFailed(tool_name.into(), format!("Metadata lookup failed: {err}"))
    })?;
    let entry_type = if metadata.file_type().is_dir() {
        "directory"
    } else if metadata.file_type().is_file() {
        "file"
    } else if metadata.file_type().is_symlink() {
        "symlink"
    } else {
        "other"
    };

    entries.push(TreeEntry {
        path: to_workspace_relative_string(tool_name, path)?,
        entry_type: entry_type.to_string(),
        depth,
    });

    if !metadata.file_type().is_dir() || depth >= max_depth {
        return Ok(());
    }

    let mut children = fs::read_dir(path)
        .map_err(|err| {
            ToolError::ExecutionFailed(tool_name.into(), format!("Read dir failed: {err}"))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            ToolError::ExecutionFailed(tool_name.into(), format!("Read dir entry failed: {err}"))
        })?;
    children.sort_by_key(|entry| entry.path());

    for child in children {
        if entries.len() >= max_entries {
            *truncated = true;
            break;
        }
        collect_tree_entries(
            tool_name,
            &child.path(),
            depth + 1,
            max_depth,
            max_entries,
            entries,
            truncated,
        )?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "tests/workspace_edit.rs"]
mod tests;
