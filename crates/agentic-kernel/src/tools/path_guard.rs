use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::config::kernel_config;

pub(crate) fn workspace_root() -> Result<PathBuf, String> {
    let workspace_dir = &kernel_config().paths.workspace_dir;
    fs::create_dir_all(workspace_dir)
        .map_err(|e| format!("SysCall Error: Failed to create workspace: {}", e))?;

    fs::canonicalize(workspace_dir)
        .map_err(|e| format!("SysCall Error: Failed to resolve workspace root: {}", e))
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

pub(crate) fn resolve_safe_path(filename: &str) -> Result<PathBuf, String> {
    let root = workspace_root()?;
    normalize_relative_path(&root, filename)
}
