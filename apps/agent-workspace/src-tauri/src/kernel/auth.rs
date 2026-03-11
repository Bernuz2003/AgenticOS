use std::path::{Path, PathBuf};

pub fn kernel_token_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join("workspace").join(".kernel_token")
}
