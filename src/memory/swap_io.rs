use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

pub(super) fn resolve_valid_swap_dir(requested: Option<PathBuf>) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("Cannot read current dir: {}", e))?;
    let workspace_root = cwd.join("workspace");
    fs::create_dir_all(&workspace_root)
        .map_err(|e| format!("Cannot create workspace dir {:?}: {}", workspace_root, e))?;

    let candidate = requested.unwrap_or_else(|| workspace_root.join("swap"));

    if !candidate.is_absolute() {
        for comp in candidate.components() {
            if matches!(comp, Component::ParentDir | Component::RootDir | Component::Prefix(_)) {
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
        cwd.join(candidate)
    };

    fs::create_dir_all(&candidate_abs)
        .map_err(|e| format!("Failed to create swap dir {:?}: {}", candidate_abs, e))?;

    let workspace_canon = fs::canonicalize(&workspace_root)
        .map_err(|e| format!("Failed to canonicalize workspace dir {:?}: {}", workspace_root, e))?;
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

pub(super) fn persist_swap_payload(
    base_dir: &Path,
    file_stem: &str,
    payload: &[u8],
) -> Result<PathBuf, String> {
    let tmp_path = base_dir.join(format!("{}.tmp", file_stem));
    let final_path = base_dir.join(format!("{}.swap", file_stem));

    if tmp_path.parent() != Some(base_dir) || final_path.parent() != Some(base_dir) {
        return Err("Swap path safety violation: computed file path escaped base dir".to_string());
    }

    let mut tmp_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .map_err(|e| format!("tmp open failed: {}", e))?;

    if let Err(e) = tmp_file.write_all(payload) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("tmp write failed: {}", e));
    }

    if let Err(e) = tmp_file.sync_all() {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("tmp fsync failed: {}", e));
    }

    drop(tmp_file);

    if let Err(e) = fs::rename(&tmp_path, &final_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("atomic rename failed: {}", e));
    }

    Ok(final_path)
}
