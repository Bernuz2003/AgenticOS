use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub(super) struct PreparedSwapTarget {
    pub tmp_path: PathBuf,
    pub final_path: PathBuf,
}

pub(super) fn resolve_valid_swap_dir(requested: Option<PathBuf>) -> Result<PathBuf, String> {
    let workspace_root = crate::config::ensure_workspace_root()?;
    let base_dir = workspace_root
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| workspace_root.clone());

    let candidate = requested.unwrap_or_else(|| workspace_root.join("swap"));

    if !candidate.is_absolute() {
        for comp in candidate.components() {
            if matches!(
                comp,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            ) {
                return Err(format!(
                    "Invalid swap path {:?}: traversal or absolute components are not allowed for relative paths",
                    candidate
                ));
            }
        }
    }

    let candidate_abs = if candidate.is_absolute() {
        candidate
    } else {
        base_dir.join(candidate)
    };

    fs::create_dir_all(&candidate_abs)
        .map_err(|e| format!("Failed to create swap dir {:?}: {}", candidate_abs, e))?;

    let workspace_canon = workspace_root;
    let candidate_canon = fs::canonicalize(&candidate_abs)
        .map_err(|e| format!("Failed to canonicalize swap dir {:?}: {}", candidate_abs, e))?;

    if !candidate_canon.starts_with(&workspace_canon) {
        return Err(format!(
            "Swap directory must be inside workspace root (workspace={:?}, requested={:?})",
            workspace_canon, candidate_canon
        ));
    }

    Ok(candidate_canon)
}

pub(super) fn prepare_swap_target(
    base_dir: &Path,
    pid: u64,
    slot_id: u64,
) -> Result<PreparedSwapTarget, String> {
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let file_stem = format!("pid_{}_slot_{}_{}", pid, slot_id, now_ns);
    let tmp_path = base_dir.join(format!("{}.tmp", file_stem));
    let final_path = base_dir.join(format!("{}.swap", file_stem));

    if tmp_path.parent() != Some(base_dir) || final_path.parent() != Some(base_dir) {
        return Err("Swap path safety violation: computed file path escaped base dir".to_string());
    }

    Ok(PreparedSwapTarget {
        tmp_path,
        final_path,
    })
}
