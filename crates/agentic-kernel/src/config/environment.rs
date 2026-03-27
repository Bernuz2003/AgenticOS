use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct ConfigBootstrapPaths {
    config_files: Vec<PathBuf>,
    env_file: PathBuf,
}

impl ConfigBootstrapPaths {
    pub fn primary_config_path(&self) -> PathBuf {
        self.config_files
            .first()
            .cloned()
            .unwrap_or_else(|| repository_path("config/kernel/base.toml"))
    }

    pub(crate) fn config_files(&self) -> &[PathBuf] {
        &self.config_files
    }

    pub(crate) fn env_file(&self) -> &Path {
        &self.env_file
    }
}

pub(crate) fn resolve_config_bootstrap_paths() -> ConfigBootstrapPaths {
    let local_override = env_string("AGENTIC_LOCAL_CONFIG_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| repository_path("config/kernel/local.toml"));
    let env_file = env_string("AGENTIC_ENV_FILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| repository_path("config/env/agenticos.env"));

    let mut config_files = if let Some(path) = env_string("AGENTIC_CONFIG_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        vec![path]
    } else {
        vec![
            repository_path("config/kernel/base.toml"),
            repository_path("agenticos.toml"),
        ]
    };

    if !config_files.contains(&local_override) {
        config_files.push(local_override);
    }

    ConfigBootstrapPaths {
        config_files,
        env_file,
    }
}

pub fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

pub fn repository_path(relative: impl AsRef<Path>) -> PathBuf {
    repository_root().join(relative)
}

#[allow(dead_code)]
pub fn env_bool(name: &str, default: bool) -> bool {
    env_bool_opt(name).unwrap_or(default)
}

#[allow(dead_code)]
pub fn env_u64(name: &str, default: u64) -> u64 {
    env_u64_opt(name).unwrap_or(default)
}

#[allow(dead_code)]
pub fn env_usize(name: &str, default: usize) -> usize {
    env_usize_opt(name).unwrap_or(default)
}

pub(crate) fn env_bool_opt(name: &str) -> Option<bool> {
    std::env::var(name).ok().map(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

pub(crate) fn env_u16(name: &str) -> Option<u16> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
}

pub(crate) fn env_u64_opt(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
}

pub(crate) fn env_f64_opt(name: &str) -> Option<f64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
}

pub(crate) fn env_usize_opt(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
}

pub(crate) fn env_string(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
}
