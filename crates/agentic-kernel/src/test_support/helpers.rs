use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) fn unique_test_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_nanos();
    format!("{prefix}-{nanos}")
}

pub(crate) fn create_temp_dir(prefix: &str) -> std::io::Result<PathBuf> {
    let path = std::env::temp_dir().join(unique_test_id(prefix));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

pub(crate) fn remove_temp_dir(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}
