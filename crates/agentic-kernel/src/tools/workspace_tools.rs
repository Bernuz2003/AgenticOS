use std::cmp::min;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agentic_kernel_macros::agentic_tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::error::ToolError;
use super::invocation::ToolContext;
use super::path_guard::{
    display_path, resolve_context_grant_roots, resolve_safe_path_for_context,
    resolve_safe_write_path_for_context,
};

pub(crate) const MAX_TEXT_FILE_BYTES: u64 = 1024 * 1024;
const DEFAULT_FIND_FILES_MAX_RESULTS: usize = 100;
const DEFAULT_SEARCH_TEXT_MAX_RESULTS: usize = 50;
const MAX_SEARCH_RESULTS_CAP: usize = 200;
const MAX_READ_FILE_RANGE_LINES: u64 = 200;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PathInfoInput {
    path: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct PathInfoOutput {
    output: String,
    path: String,
    exists: bool,
    entry_type: Option<String>,
    size_bytes: Option<u64>,
    modified_unix_ms: Option<u64>,
}

#[agentic_tool(
    name = "path_info",
    description = "Inspect metadata for a file or directory inside the process-scoped workspace.",
    input_example = serde_json::json!({"path": "crates/agentic-kernel/src/tools"}),
    capabilities = ["fs", "metadata"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn path_info(input: PathInfoInput, ctx: &ToolContext) -> Result<PathInfoOutput, ToolError> {
    ensure_non_empty_path("path_info", &input.path)?;
    let path = resolve_safe_path_for_context(&input.path, ctx)
        .map_err(|err| ToolError::ExecutionFailed("path_info".into(), err))?;

    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PathInfoOutput {
                output: format!("Path '{}' does not exist.", input.path),
                path: input.path,
                exists: false,
                entry_type: None,
                size_bytes: None,
                modified_unix_ms: None,
            });
        }
        Err(err) => {
            return Err(ToolError::ExecutionFailed(
                "path_info".into(),
                format!("Metadata lookup failed: {err}"),
            ))
        }
    };

    let entry_type = if metadata.file_type().is_dir() {
        Some("directory".to_string())
    } else if metadata.file_type().is_file() {
        Some("file".to_string())
    } else if metadata.file_type().is_symlink() {
        Some("symlink".to_string())
    } else {
        Some("other".to_string())
    };

    let size_bytes = metadata.file_type().is_file().then_some(metadata.len());
    let modified_unix_ms = metadata.modified().ok().and_then(system_time_to_unix_ms);

    let output = match entry_type.as_deref() {
        Some("file") => format!(
            "{}: file ({} bytes)",
            input.path,
            size_bytes.unwrap_or_default()
        ),
        Some(kind) => format!("{}: {}", input.path, kind),
        None => format!("{}: unknown", input.path),
    };

    Ok(PathInfoOutput {
        output,
        path: input.path,
        exists: true,
        entry_type,
        size_bytes,
        modified_unix_ms,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FindFilesInput {
    path: Option<String>,
    pattern: String,
    extension: Option<String>,
    recursive: Option<bool>,
    max_results: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct FindFilesOutput {
    output: String,
    root: String,
    matches: Vec<String>,
    truncated: bool,
}

#[agentic_tool(
    name = "find_files",
    description = "Find files inside the process-scoped workspace using a required filename pattern and an optional extension filter.",
    input_example = serde_json::json!({"pattern": "agent", "path": "crates", "extension": "rs", "recursive": true}),
    capabilities = ["fs", "search"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn find_files(input: FindFilesInput, ctx: &ToolContext) -> Result<FindFilesOutput, ToolError> {
    let root = resolve_search_root("find_files", input.path.as_deref(), ctx)?;
    let recursive = input.recursive.unwrap_or(true);
    let max_results = normalize_max_results(input.max_results, DEFAULT_FIND_FILES_MAX_RESULTS);
    let pattern = input.pattern.trim();
    if pattern.is_empty() {
        return Err(ToolError::InvalidInput(
            "find_files".into(),
            "field 'pattern' cannot be empty".into(),
        ));
    }
    let extension = input
        .extension
        .as_deref()
        .map(normalize_extension)
        .filter(|value| !value.is_empty());

    let mut matches = Vec::new();
    let mut truncated = false;
    for path in collect_candidate_files("find_files", &root.absolute, recursive)? {
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if !filename_matches(file_name, Some(pattern), extension.as_deref()) {
            continue;
        }

        matches.push(to_workspace_relative_string("find_files", &path)?);
        if matches.len() >= max_results {
            truncated = true;
            break;
        }
    }

    let output = if matches.is_empty() {
        format!("No files matched under '{}'.", root.display)
    } else {
        format!(
            "Found files under '{}':\n- {}",
            root.display,
            matches.join("\n- ")
        )
    };

    Ok(FindFilesOutput {
        output,
        root: root.display,
        matches,
        truncated,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SearchTextInput {
    query: String,
    path: Option<String>,
    recursive: Option<bool>,
    case_sensitive: Option<bool>,
    max_results: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct SearchTextMatch {
    path: String,
    line: u64,
    column: u64,
    text: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct SearchTextOutput {
    output: String,
    query: String,
    root: String,
    matches: Vec<SearchTextMatch>,
    truncated: bool,
}

#[agentic_tool(
    name = "search_text",
    description = "Search plain text across UTF-8 files inside the process-scoped workspace.",
    input_example = serde_json::json!({"query": "ToolRegistry", "path": "crates/agentic-kernel/src", "recursive": true, "case_sensitive": true}),
    capabilities = ["fs", "search", "text"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn search_text(input: SearchTextInput, ctx: &ToolContext) -> Result<SearchTextOutput, ToolError> {
    if input.query.trim().is_empty() {
        return Err(ToolError::InvalidInput(
            "search_text".into(),
            "field 'query' cannot be empty".into(),
        ));
    }

    let root = resolve_search_root("search_text", input.path.as_deref(), ctx)?;
    let recursive = input.recursive.unwrap_or(true);
    let case_sensitive = input.case_sensitive.unwrap_or(true);
    let max_results = normalize_max_results(input.max_results, DEFAULT_SEARCH_TEXT_MAX_RESULTS);
    let mut matches = Vec::new();
    let mut truncated = false;

    for path in collect_candidate_files("search_text", &root.absolute, recursive)? {
        let Some(content) = read_optional_utf8_file("search_text", &path, MAX_TEXT_FILE_BYTES)?
        else {
            continue;
        };

        for (index, line) in content.lines().enumerate() {
            let column = find_query_column(line, &input.query, case_sensitive);
            if let Some(column) = column {
                matches.push(SearchTextMatch {
                    path: to_workspace_relative_string("search_text", &path)?,
                    line: (index + 1) as u64,
                    column,
                    text: line.to_string(),
                });
                if matches.len() >= max_results {
                    truncated = true;
                    break;
                }
            }
        }

        if truncated {
            break;
        }
    }

    let summary_lines: Vec<String> = matches
        .iter()
        .map(|entry| {
            format!(
                "{}:{}:{}: {}",
                entry.path, entry.line, entry.column, entry.text
            )
        })
        .collect();
    let output = if summary_lines.is_empty() {
        format!("No matches for '{}' under '{}'.", input.query, root.display)
    } else {
        format!(
            "Matches for '{}' under '{}':\n{}",
            input.query,
            root.display,
            summary_lines.join("\n")
        )
    };

    Ok(SearchTextOutput {
        output,
        query: input.query,
        root: root.display,
        matches,
        truncated,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadFileRangeInput {
    path: String,
    start_line: u64,
    end_line: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct ReadFileRangeLine {
    number: u64,
    text: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct ReadFileRangeOutput {
    output: String,
    path: String,
    start_line: u64,
    end_line: u64,
    lines: Vec<ReadFileRangeLine>,
}

#[agentic_tool(
    name = "read_file_range",
    description = "Read an inclusive line range from a UTF-8 text file inside the process-scoped workspace.",
    input_example = serde_json::json!({"path": "docs/MILESTONE.md", "start_line": 1, "end_line": 20}),
    capabilities = ["fs", "read", "range"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn read_file_range(
    input: ReadFileRangeInput,
    ctx: &ToolContext,
) -> Result<ReadFileRangeOutput, ToolError> {
    ensure_non_empty_path("read_file_range", &input.path)?;
    if input.start_line == 0 {
        return Err(ToolError::InvalidInput(
            "read_file_range".into(),
            "'start_line' must be >= 1".into(),
        ));
    }

    let end_line = input.end_line.unwrap_or(input.start_line);
    if end_line < input.start_line {
        return Err(ToolError::InvalidInput(
            "read_file_range".into(),
            "'end_line' must be >= 'start_line'".into(),
        ));
    }
    if end_line - input.start_line + 1 > MAX_READ_FILE_RANGE_LINES {
        return Err(ToolError::InvalidInput(
            "read_file_range".into(),
            format!(
                "requested range is too large; maximum supported line span is {}",
                MAX_READ_FILE_RANGE_LINES
            ),
        ));
    }

    let path = resolve_safe_path_for_context(&input.path, ctx)
        .map_err(|err| ToolError::ExecutionFailed("read_file_range".into(), err))?;
    let content = read_required_utf8_file("read_file_range", &path, MAX_TEXT_FILE_BYTES)?;

    let lines: Vec<ReadFileRangeLine> = content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line_number = (index + 1) as u64;
            (line_number >= input.start_line && line_number <= end_line).then(|| {
                ReadFileRangeLine {
                    number: line_number,
                    text: line.to_string(),
                }
            })
        })
        .collect();

    let output = if lines.is_empty() {
        format!(
            "No lines found for '{}' in the requested range {}-{}.",
            input.path, input.start_line, end_line
        )
    } else {
        lines
            .iter()
            .map(|line| format!("{:>4}: {}", line.number, line.text))
            .collect::<Vec<_>>()
            .join("\n")
    };

    Ok(ReadFileRangeOutput {
        output,
        path: input.path,
        start_line: input.start_line,
        end_line,
        lines,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MkdirInput {
    path: String,
    create_parents: Option<bool>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct MkdirOutput {
    output: String,
    path: String,
    created: bool,
}

#[agentic_tool(
    name = "mkdir",
    description = "Create a directory inside the process-scoped workspace.",
    input_example = serde_json::json!({"path": "reports/daily", "create_parents": true}),
    capabilities = ["fs", "mkdir"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn mkdir(input: MkdirInput, ctx: &ToolContext) -> Result<MkdirOutput, ToolError> {
    ensure_non_empty_path("mkdir", &input.path)?;
    let path = resolve_safe_write_path_for_context(&input.path, ctx)
        .map_err(|err| ToolError::ExecutionFailed("mkdir".into(), err))?;

    if path.exists() {
        if path.is_dir() {
            return Ok(MkdirOutput {
                output: format!("Directory '{}' already exists.", input.path),
                path: input.path,
                created: false,
            });
        }
        return Err(ToolError::ExecutionFailed(
            "mkdir".into(),
            format!(
                "Path '{}' already exists and is not a directory.",
                input.path
            ),
        ));
    }

    let create_parents = input.create_parents.unwrap_or(true);
    let result = if create_parents {
        fs::create_dir_all(&path)
    } else {
        fs::create_dir(&path)
    };
    result.map_err(|err| {
        ToolError::ExecutionFailed("mkdir".into(), format!("mkdir failed: {err}"))
    })?;

    Ok(MkdirOutput {
        output: format!("Directory '{}' created.", input.path),
        path: input.path,
        created: true,
    })
}

pub(crate) struct SearchRoot {
    pub(crate) absolute: PathBuf,
    pub(crate) display: String,
}

pub(crate) fn ensure_non_empty_path(tool_name: &str, path: &str) -> Result<(), ToolError> {
    if path.trim().is_empty() {
        Err(ToolError::InvalidInput(
            tool_name.into(),
            "field 'path' cannot be empty".into(),
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn resolve_search_root(
    tool_name: &str,
    path: Option<&str>,
    ctx: &ToolContext,
) -> Result<SearchRoot, ToolError> {
    match path.map(str::trim).filter(|value| !value.is_empty()) {
        Some(path) => {
            let absolute = resolve_safe_path_for_context(path, ctx)
                .map_err(|err| ToolError::ExecutionFailed(tool_name.into(), err))?;
            if !absolute.exists() {
                return Err(ToolError::ExecutionFailed(
                    tool_name.into(),
                    format!("Path '{}' does not exist.", path),
                ));
            }
            Ok(SearchRoot {
                absolute,
                display: path.to_string(),
            })
        }
        None => default_scoped_search_root(tool_name, ctx),
    }
}

pub(crate) fn default_scoped_search_root(
    tool_name: &str,
    ctx: &ToolContext,
) -> Result<SearchRoot, ToolError> {
    let roots = resolve_context_grant_roots(ctx)
        .map_err(|err| ToolError::ExecutionFailed(tool_name.into(), err))?;
    match roots.as_slice() {
        [] => Err(ToolError::PolicyDenied(
            tool_name.into(),
            "no path grants are available for this process".into(),
        )),
        [root] => Ok(SearchRoot {
            absolute: root.clone(),
            display: to_workspace_relative_string(tool_name, root)?,
        }),
        _ => Err(ToolError::InvalidInput(
            tool_name.into(),
            "this process is scoped to multiple roots; provide an explicit 'path'".into(),
        )),
    }
}

fn normalize_max_results(input: Option<u64>, default: usize) -> usize {
    let raw = input.unwrap_or(default as u64);
    let raw = min(raw, MAX_SEARCH_RESULTS_CAP as u64);
    raw.max(1) as usize
}

fn collect_candidate_files(
    tool_name: &str,
    base: &Path,
    recursive: bool,
) -> Result<Vec<PathBuf>, ToolError> {
    if base.is_file() {
        return Ok(vec![base.to_path_buf()]);
    }
    if !base.is_dir() {
        return Err(ToolError::ExecutionFailed(
            tool_name.into(),
            format!("Path '{}' is not a file or directory.", base.display()),
        ));
    }

    let mut directories = vec![base.to_path_buf()];
    let mut files = Vec::new();

    while let Some(dir) = directories.pop() {
        let entries = fs::read_dir(&dir).map_err(|err| {
            ToolError::ExecutionFailed(tool_name.into(), format!("Failed to read directory: {err}"))
        })?;
        for entry in entries {
            let entry = entry.map_err(|err| {
                ToolError::ExecutionFailed(
                    tool_name.into(),
                    format!("Failed to read directory entry: {err}"),
                )
            })?;
            let file_type = entry.file_type().map_err(|err| {
                ToolError::ExecutionFailed(
                    tool_name.into(),
                    format!("Failed to read entry type: {err}"),
                )
            })?;
            let path = entry.path();
            if file_type.is_file() {
                files.push(path);
            } else if file_type.is_dir() && recursive {
                directories.push(path);
            }
        }
    }

    files.sort();
    Ok(files)
}

pub(crate) fn read_optional_utf8_file(
    tool_name: &str,
    path: &Path,
    max_bytes: u64,
) -> Result<Option<String>, ToolError> {
    let metadata = fs::metadata(path).map_err(|err| {
        ToolError::ExecutionFailed(tool_name.into(), format!("Metadata lookup failed: {err}"))
    })?;
    if metadata.len() > max_bytes {
        return Ok(None);
    }

    let bytes = fs::read(path).map_err(|err| {
        ToolError::ExecutionFailed(tool_name.into(), format!("Read failed: {err}"))
    })?;
    match String::from_utf8(bytes) {
        Ok(content) => Ok(Some(content)),
        Err(_) => Ok(None),
    }
}

pub(crate) fn read_required_utf8_file(
    tool_name: &str,
    path: &Path,
    max_bytes: u64,
) -> Result<String, ToolError> {
    let metadata = fs::metadata(path).map_err(|err| {
        ToolError::ExecutionFailed(tool_name.into(), format!("Metadata lookup failed: {err}"))
    })?;
    if metadata.len() > max_bytes {
        return Err(ToolError::ExecutionFailed(
            tool_name.into(),
            format!("Refusing to read files larger than {} bytes.", max_bytes),
        ));
    }

    fs::read_to_string(path)
        .map_err(|err| ToolError::ExecutionFailed(tool_name.into(), format!("Read failed: {err}")))
}

pub(crate) fn to_workspace_relative_string(
    tool_name: &str,
    path: &Path,
) -> Result<String, ToolError> {
    display_path(path).map_err(|err| ToolError::ExecutionFailed(tool_name.into(), err))
}

fn filename_matches(file_name: &str, pattern: Option<&str>, extension: Option<&str>) -> bool {
    let normalized_name = file_name.to_ascii_lowercase();
    let pattern_matches =
        pattern.is_none_or(|pattern| normalized_name.contains(&pattern.to_ascii_lowercase()));
    let extension_matches = extension.is_none_or(|extension| {
        Path::new(file_name)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case(extension))
            .unwrap_or(false)
    });

    pattern_matches && extension_matches
}

fn normalize_extension(extension: &str) -> String {
    extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

fn find_query_column(line: &str, query: &str, case_sensitive: bool) -> Option<u64> {
    if case_sensitive {
        line.find(query).map(|index| index as u64 + 1)
    } else {
        let line_lower = line.to_ascii_lowercase();
        let query_lower = query.to_ascii_lowercase();
        line_lower.find(&query_lower).map(|index| index as u64 + 1)
    }
}

fn system_time_to_unix_ms(value: SystemTime) -> Option<u64> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

#[cfg(test)]
#[path = "tests/workspace.rs"]
mod tests;
