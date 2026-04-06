use std::path::{Component, Path, PathBuf};

use crate::config::ensure_workspace_root;
use crate::tools::invocation::ToolContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathAccessIntent {
    Read,
    Write,
}

pub(crate) fn workspace_root() -> Result<PathBuf, String> {
    ensure_workspace_root().map_err(|e| format!("SysCall Error: {}", e))
}

pub(crate) fn normalize_relative_path(root: &Path, input: &str) -> Result<PathBuf, String> {
    let clean_input = input.trim();
    if clean_input.is_empty() {
        return Err("SysCall Error: Empty filename.".to_string());
    }
    if clean_input.contains('\0') {
        return Err("SysCall Error: Invalid filename (contains NUL).".to_string());
    }

    let candidate = Path::new(clean_input);
    if candidate.is_absolute() {
        return Err("SysCall Error: Absolute paths are not allowed.".to_string());
    }

    let mut out = root.to_path_buf();
    for comp in candidate.components() {
        match comp {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            Component::ParentDir => {
                if out == root {
                    return Err("SysCall Error: Path traversal denied.".to_string());
                }
                out.pop();
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("SysCall Error: Invalid path root/prefix.".to_string());
            }
        }
    }

    if !out.starts_with(root) {
        return Err("SysCall Error: Path escapes workspace.".to_string());
    }

    Ok(out)
}

pub(crate) fn resolve_safe_path_for_context(
    filename: &str,
    context: &ToolContext,
) -> Result<PathBuf, String> {
    resolve_safe_read_path_for_context(filename, context)
}

pub(crate) fn resolve_safe_read_path_for_context(
    filename: &str,
    context: &ToolContext,
) -> Result<PathBuf, String> {
    let root = workspace_root()?;
    let resolved = normalize_input_path(&root, filename)?;
    ensure_path_access(&root, &resolved, context, PathAccessIntent::Read)?;
    Ok(resolved)
}

pub(crate) fn resolve_safe_write_path_for_context(
    filename: &str,
    context: &ToolContext,
) -> Result<PathBuf, String> {
    let root = workspace_root()?;
    let resolved = normalize_input_path(&root, filename)?;
    ensure_path_access(&root, &resolved, context, PathAccessIntent::Write)?;
    Ok(resolved)
}

pub(crate) fn resolve_context_grant_roots(context: &ToolContext) -> Result<Vec<PathBuf>, String> {
    let root = workspace_root()?;
    if context.permissions.path_grants.is_empty() {
        return Err("SysCall Error: No path grants are available for this process.".to_string());
    }

    let mut roots = Vec::new();
    for grant in &context.permissions.path_grants {
        let absolute = absolute_grant_root(&root, &grant.root)?;
        if !roots.contains(&absolute) {
            roots.push(absolute);
        }
    }
    Ok(roots)
}

pub(crate) fn display_path(path: &Path) -> Result<String, String> {
    let root = workspace_root()?;
    if let Ok(relative) = path.strip_prefix(&root) {
        let text = relative.to_string_lossy().replace('\\', "/");
        return Ok(if text.is_empty() {
            ".".to_string()
        } else {
            text
        });
    }

    Ok(path.to_string_lossy().replace('\\', "/"))
}

pub(crate) fn ensure_path_access(
    root: &Path,
    candidate: &Path,
    context: &ToolContext,
    intent: PathAccessIntent,
) -> Result<(), String> {
    if context.permissions.path_grants.is_empty() {
        return Err("SysCall Error: No path grants are available for this process.".to_string());
    }

    let mut matching_read_only = Vec::new();

    for grant in &context.permissions.path_grants {
        let allowed_root = absolute_grant_root(root, &grant.root)?;
        if candidate.starts_with(&allowed_root) {
            if intent == PathAccessIntent::Read || grant.allows_write() {
                return Ok(());
            }
            matching_read_only.push(grant.root.clone());
        }
    }

    let candidate_display = display_candidate_against_workspace(root, candidate);
    if !matching_read_only.is_empty() {
        return Err(format!(
            "SysCall Error: Path '{}' is read-only under grants [{}].",
            candidate_display,
            matching_read_only.join(", ")
        ));
    }

    Err(format!(
        "SysCall Error: Path '{}' is outside allowed grants [{}].",
        candidate_display,
        context.permissions.path_scopes.join(", ")
    ))
}

fn normalize_input_path(root: &Path, input: &str) -> Result<PathBuf, String> {
    let clean_input = input.trim();
    if clean_input.is_empty() {
        return Err("SysCall Error: Empty filename.".to_string());
    }
    if clean_input.contains('\0') {
        return Err("SysCall Error: Invalid filename (contains NUL).".to_string());
    }

    let candidate = Path::new(clean_input);
    if candidate.is_absolute() {
        normalize_absolute_path(candidate)
    } else {
        normalize_relative_path(root, clean_input)
    }
}

fn normalize_absolute_path(candidate: &Path) -> Result<PathBuf, String> {
    let mut out = PathBuf::new();
    let mut saw_root = false;
    let mut depth = 0usize;

    for comp in candidate.components() {
        match comp {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => {
                out.push(std::path::MAIN_SEPARATOR_STR);
                saw_root = true;
            }
            Component::Normal(seg) => {
                out.push(seg);
                depth += 1;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if depth == 0 || !out.pop() {
                    return Err("SysCall Error: Absolute path escapes filesystem root.".to_string());
                }
                depth -= 1;
            }
        }
    }

    if candidate.is_absolute() && !saw_root && out.as_os_str().is_empty() {
        return Err("SysCall Error: Invalid absolute path.".to_string());
    }

    Ok(out)
}

fn absolute_grant_root(root: &Path, scope: &str) -> Result<PathBuf, String> {
    if scope == "." {
        return Ok(root.to_path_buf());
    }

    let scope_path = Path::new(scope);
    if scope_path.is_absolute() {
        normalize_absolute_path(scope_path)
    } else {
        normalize_relative_path(root, scope)
    }
}

fn display_candidate_against_workspace(root: &Path, candidate: &Path) -> String {
    if let Ok(relative) = candidate.strip_prefix(root) {
        let text = relative.to_string_lossy().replace('\\', "/");
        if text.is_empty() {
            return ".".to_string();
        }
        return text;
    }

    candidate.to_string_lossy().replace('\\', "/")
}
