use super::kernel_config;
use super::models::*;
/// Parsing and environment overrides for configuration.
use std::fs;
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;

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
}

pub(crate) fn load_kernel_config() -> Result<KernelConfig, String> {
    let bootstrap = resolve_config_bootstrap_paths();
    let primary_config_path = bootstrap.primary_config_path();
    let merged = load_merged_toml_config(&bootstrap.config_files)?;
    let mut config = if let Some(value) = merged {
        value.try_into::<KernelConfig>().map_err(|e| {
            format!(
                "invalid merged config rooted at '{}': {}",
                primary_config_path.display(),
                e
            )
        })?
    } else {
        KernelConfig::default()
    };

    load_env_file(&bootstrap.env_file)?;
    apply_env_overrides(&mut config);
    normalize_config_paths(&mut config, &primary_config_path);
    Ok(config)
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

pub(crate) fn load_merged_toml_config(
    config_files: &[PathBuf],
) -> Result<Option<TomlValue>, String> {
    let mut merged: Option<TomlValue> = None;

    for path in config_files {
        if !path.exists() {
            continue;
        }

        let raw = fs::read_to_string(path)
            .map_err(|e| format!("failed to read config '{}': {}", path.display(), e))?;
        let value = toml::from_str::<TomlValue>(&raw)
            .map_err(|e| format!("invalid config '{}': {}", path.display(), e))?;

        if let Some(current) = merged.as_mut() {
            merge_toml_value(current, value);
        } else {
            merged = Some(value);
        }
    }

    Ok(merged)
}

pub(crate) fn merge_toml_value(base: &mut TomlValue, overlay: TomlValue) {
    match (base, overlay) {
        (TomlValue::Table(base_table), TomlValue::Table(overlay_table)) => {
            for (key, overlay_value) in overlay_table {
                if let Some(base_value) = base_table.get_mut(&key) {
                    merge_toml_value(base_value, overlay_value);
                } else {
                    base_table.insert(key, overlay_value);
                }
            }
        }
        (base_value, overlay_value) => {
            *base_value = overlay_value;
        }
    }
}

pub(crate) fn load_env_file(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    let raw = fs::read_to_string(path)
        .map_err(|e| format!("failed to read env file '{}': {}", path.display(), e))?;

    for (index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let entry = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let (key, value) = entry.split_once('=').ok_or_else(|| {
            format!(
                "invalid env file '{}': line {} must be KEY=VALUE",
                path.display(),
                index + 1
            )
        })?;
        let key = key.trim();
        if key.is_empty() {
            return Err(format!(
                "invalid env file '{}': line {} has empty key",
                path.display(),
                index + 1
            ));
        }

        if std::env::var_os(key).is_some() {
            continue;
        }

        std::env::set_var(key, parse_env_value(value.trim()));
    }

    Ok(())
}

pub(crate) fn parse_env_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let quoted = (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''));
        if quoted {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }

    trimmed.to_string()
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

pub fn ensure_workspace_root() -> Result<PathBuf, String> {
    let workspace_dir = &kernel_config().paths.workspace_dir;
    fs::create_dir_all(workspace_dir).map_err(|e| {
        format!(
            "Failed to create workspace root '{}': {}",
            workspace_dir.display(),
            e
        )
    })?;

    fs::canonicalize(workspace_dir).map_err(|e| {
        format!(
            "Failed to resolve workspace root '{}': {}",
            workspace_dir.display(),
            e
        )
    })
}

pub(crate) fn normalize_config_paths(config: &mut KernelConfig, config_path: &Path) {
    let base_dir = if config_path.is_absolute() {
        config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(repository_root)
    } else if config_path.exists() {
        std::env::current_dir()
            .ok()
            .map(|cwd| cwd.join(config_path))
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(repository_root)
    } else {
        repository_root()
    };

    absolutize_from(&base_dir, &mut config.paths.models_dir);
    absolutize_from(&base_dir, &mut config.paths.workspace_dir);
    absolutize_from(&base_dir, &mut config.paths.database_path);
    absolutize_from(&base_dir, &mut config.paths.checkpoint_path);
    absolutize_from(&base_dir, &mut config.paths.kernel_token_path);
    absolutize_from(&base_dir, &mut config.paths.remote_provider_catalog_path);
    absolutize_from(&base_dir, &mut config.memory.swap_dir);
    absolutize_remote_tokenizer_path(&base_dir, &mut config.openai_responses.tokenizer_path);
    absolutize_remote_tokenizer_path(&base_dir, &mut config.groq_responses.tokenizer_path);
    absolutize_remote_tokenizer_path(&base_dir, &mut config.openrouter.tokenizer_path);
}

pub(crate) fn absolutize_from(base_dir: &Path, path: &mut PathBuf) {
    if path.is_relative() {
        *path = base_dir.join(&*path);
    }
}

pub(crate) fn absolutize_remote_tokenizer_path(base_dir: &Path, path: &mut Option<PathBuf>) {
    if let Some(path) = path.as_mut() {
        absolutize_from(base_dir, path);
    }
}

pub(crate) fn apply_env_overrides(config: &mut KernelConfig) {
    if let Some(value) = env_u16("AGENTIC_PORT") {
        config.network.port = value;
    }
    if let Some(value) = env_bool_opt("AGENTIC_LOG_CONNECTIONS") {
        config.network.log_connections = value;
    }
    if let Some(value) = env_bool_opt("AGENTIC_PROTOCOL_DEFAULT_V1") {
        config.protocol.default_contract_v1 = value;
    }
    if let Some(value) = env_bool_opt("AGENTIC_PROTOCOL_ALLOW_LEGACY") {
        config.protocol.allow_legacy_fallback = value;
    }
    if let Some(value) = env_string("AGENTIC_REMOTE_PROVIDERS_PATH") {
        config.paths.remote_provider_catalog_path = PathBuf::from(value);
    }
    if let Some(value) = env_string("AGENTIC_DB_PATH") {
        config.paths.database_path = PathBuf::from(value);
    }
    if let Some(value) = env_string("AGENTIC_PROTOCOL_CAPABILITIES") {
        let capabilities: Vec<String> = value
            .split(',')
            .map(|item| item.trim())
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect();
        if !capabilities.is_empty() {
            config.protocol.enabled_capabilities = capabilities;
        }
    }
    if let Some(value) = env_bool_opt("AGENTIC_MEMORY_SWAP_ASYNC") {
        config.memory.swap_async = value;
    }
    if let Some(value) = env_string("AGENTIC_MEMORY_SWAP_DIR") {
        config.memory.swap_dir = PathBuf::from(value);
    }
    if let Some(value) = env_string("AGENTIC_CONTEXT_DEFAULT_STRATEGY") {
        config.context.default_strategy = value;
    }
    if let Some(value) = env_usize_opt("AGENTIC_CONTEXT_WINDOW_TOKENS") {
        config.context.default_window_tokens = value.max(1);
    }
    if let Some(value) = env_usize_opt("AGENTIC_CONTEXT_TRIGGER_TOKENS") {
        config.context.compaction_trigger_tokens = value.max(1);
    }
    if let Some(value) = env_usize_opt("AGENTIC_CONTEXT_TARGET_TOKENS") {
        config.context.compaction_target_tokens = value.max(1);
    }
    if let Some(value) = env_usize_opt("AGENTIC_CONTEXT_RETRIEVE_TOP_K") {
        config.context.retrieve_top_k = value.max(1);
    }
    if let Some(value) = env_usize_opt("AGENTIC_CONTEXT_RETRIEVE_CANDIDATE_LIMIT") {
        config.context.retrieve_candidate_limit = value.max(1);
    }
    if let Some(value) = env_usize_opt("AGENTIC_CONTEXT_RETRIEVE_MAX_SEGMENT_CHARS") {
        config.context.retrieve_max_segment_chars = value.max(64);
    }
    if let Some(value) = env_f64_opt("AGENTIC_CONTEXT_RETRIEVE_MIN_SCORE") {
        config.context.retrieve_min_score = value.max(0.0);
    }
    if let Some(value) = env_u64_opt("AGENTIC_CHECKPOINT_INTERVAL_SECS") {
        config.checkpoint.interval_secs = value;
    }
    if let Some(value) = env_bool_opt("AGENTIC_AUTH_DISABLED") {
        config.auth.disabled = value;
    }
    if let Some(value) = env_string("AGENTIC_LLAMACPP_EXECUTABLE") {
        config.external_llamacpp.executable = value;
    }
    if let Some(value) = env_u16("AGENTIC_LLAMACPP_PORT_BASE") {
        config.external_llamacpp.port_base = value;
    }
    if let Some(value) = env_u64_opt("AGENTIC_LLAMACPP_STARTUP_TIMEOUT_MS") {
        config.external_llamacpp.startup_timeout_ms = value.max(1);
    }
    if let Some(value) = env_u64_opt("AGENTIC_LLAMACPP_HEALTH_POLL_MS") {
        config.external_llamacpp.health_poll_ms = value.max(10);
    }
    if let Some(value) = env_string("AGENTIC_LLAMACPP_ENDPOINT") {
        config.external_llamacpp.legacy_endpoint_override = value;
    }
    if let Some(value) = env_u64_opt("AGENTIC_LLAMACPP_TIMEOUT_MS") {
        config.external_llamacpp.timeout_ms = value;
    }
    if let Some(value) = env_usize_opt("AGENTIC_LLAMACPP_CHUNK_TOKENS") {
        config.external_llamacpp.chunk_tokens = value.max(1);
    }
    if let Some(value) = env_string("AGENTIC_OPENAI_ENDPOINT") {
        config.openai_responses.endpoint = value;
    }
    if let Some(value) = env_string("AGENTIC_OPENAI_API_KEY") {
        config.openai_responses.api_key = value;
    } else if let Some(value) = env_string("OPENAI_API_KEY") {
        config.openai_responses.api_key = value;
    }
    if let Some(value) = env_string("AGENTIC_OPENAI_DEFAULT_MODEL") {
        config.openai_responses.default_model = value;
    }
    if let Some(value) = env_u64_opt("AGENTIC_OPENAI_TIMEOUT_MS") {
        config.openai_responses.timeout_ms = value.max(1);
    }
    if let Some(value) = env_usize_opt("AGENTIC_OPENAI_MAX_REQUEST_BYTES") {
        config.openai_responses.max_request_bytes = value.max(1024);
    }
    if let Some(value) = env_usize_opt("AGENTIC_OPENAI_MAX_RESPONSE_BYTES") {
        config.openai_responses.max_response_bytes = value.max(1024);
    }
    if let Some(value) = env_bool_opt("AGENTIC_OPENAI_STREAM") {
        config.openai_responses.stream = value;
    }
    if let Some(value) = env_string("AGENTIC_OPENAI_TOKENIZER_PATH") {
        config.openai_responses.tokenizer_path = Some(PathBuf::from(value));
    }
    if let Some(value) = env_f64_opt("AGENTIC_OPENAI_INPUT_PRICE_USD_PER_MTOK") {
        config.openai_responses.input_price_usd_per_mtok = value.max(0.0);
    }
    if let Some(value) = env_f64_opt("AGENTIC_OPENAI_OUTPUT_PRICE_USD_PER_MTOK") {
        config.openai_responses.output_price_usd_per_mtok = value.max(0.0);
    }
    if let Some(value) = env_string("AGENTIC_GROQ_ENDPOINT") {
        config.groq_responses.endpoint = value;
    }
    if let Some(value) = env_string("AGENTIC_GROQ_API_KEY") {
        config.groq_responses.api_key = value;
    } else if let Some(value) = env_string("GROQ_API_KEY") {
        config.groq_responses.api_key = value;
    }
    if let Some(value) = env_string("AGENTIC_GROQ_DEFAULT_MODEL") {
        config.groq_responses.default_model = value;
    }
    if let Some(value) = env_u64_opt("AGENTIC_GROQ_TIMEOUT_MS") {
        config.groq_responses.timeout_ms = value.max(1);
    }
    if let Some(value) = env_usize_opt("AGENTIC_GROQ_MAX_REQUEST_BYTES") {
        config.groq_responses.max_request_bytes = value.max(1024);
    }
    if let Some(value) = env_usize_opt("AGENTIC_GROQ_MAX_RESPONSE_BYTES") {
        config.groq_responses.max_response_bytes = value.max(1024);
    }
    if let Some(value) = env_bool_opt("AGENTIC_GROQ_STREAM") {
        config.groq_responses.stream = value;
    }
    if let Some(value) = env_string("AGENTIC_GROQ_TOKENIZER_PATH") {
        config.groq_responses.tokenizer_path = Some(PathBuf::from(value));
    }
    if let Some(value) = env_f64_opt("AGENTIC_GROQ_INPUT_PRICE_USD_PER_MTOK") {
        config.groq_responses.input_price_usd_per_mtok = value.max(0.0);
    }
    if let Some(value) = env_f64_opt("AGENTIC_GROQ_OUTPUT_PRICE_USD_PER_MTOK") {
        config.groq_responses.output_price_usd_per_mtok = value.max(0.0);
    }
    if let Some(value) = env_string("AGENTIC_OPENROUTER_ENDPOINT") {
        config.openrouter.endpoint = value;
    }
    if let Some(value) = env_string("AGENTIC_OPENROUTER_API_KEY") {
        config.openrouter.api_key = value;
    } else if let Some(value) = env_string("OPENROUTER_API_KEY") {
        config.openrouter.api_key = value;
    }
    if let Some(value) = env_string("AGENTIC_OPENROUTER_DEFAULT_MODEL") {
        config.openrouter.default_model = value;
    }
    if let Some(value) = env_u64_opt("AGENTIC_OPENROUTER_TIMEOUT_MS") {
        config.openrouter.timeout_ms = value.max(1);
    }
    if let Some(value) = env_usize_opt("AGENTIC_OPENROUTER_MAX_REQUEST_BYTES") {
        config.openrouter.max_request_bytes = value.max(1024);
    }
    if let Some(value) = env_usize_opt("AGENTIC_OPENROUTER_MAX_RESPONSE_BYTES") {
        config.openrouter.max_response_bytes = value.max(1024);
    }
    if let Some(value) = env_bool_opt("AGENTIC_OPENROUTER_STREAM") {
        config.openrouter.stream = value;
    }
    if let Some(value) = env_string("AGENTIC_OPENROUTER_TOKENIZER_PATH") {
        config.openrouter.tokenizer_path = Some(PathBuf::from(value));
    }
    if let Some(value) = env_f64_opt("AGENTIC_OPENROUTER_INPUT_PRICE_USD_PER_MTOK") {
        config.openrouter.input_price_usd_per_mtok = value.max(0.0);
    }
    if let Some(value) = env_f64_opt("AGENTIC_OPENROUTER_OUTPUT_PRICE_USD_PER_MTOK") {
        config.openrouter.output_price_usd_per_mtok = value.max(0.0);
    }
    if let Some(value) = env_string("AGENTIC_OPENROUTER_HTTP_REFERER") {
        config.openrouter.http_referer = value;
    }
    if let Some(value) = env_string("AGENTIC_OPENROUTER_TITLE") {
        config.openrouter.app_title = value;
    }
    if let Some(value) = env_bool_opt("AGENTIC_EXEC_AUTO_SWITCH") {
        config.exec.auto_switch = value;
    }
    if let Some(value) = env_usize_opt("AGENTIC_ORCH_MAX_OUTPUT_CHARS") {
        config.orchestrator.max_output_chars = value.max(1);
    }
    if let Some(value) = env_string("AGENTIC_SANDBOX_MODE") {
        config.tools.sandbox_mode = value;
    }
    if let Some(value) = env_bool_opt("AGENTIC_ALLOW_HOST_FALLBACK") {
        config.tools.allow_host_fallback = value;
    }
    if let Some(value) = env_u64_opt("AGENTIC_SYSCALL_TIMEOUT_S") {
        config.tools.timeout_s = value.max(1);
    }
    if let Some(value) = env_usize_opt("AGENTIC_SYSCALL_MAX_PER_WINDOW") {
        config.tools.max_calls_per_window = value.max(1);
    }
    if let Some(value) = env_u64_opt("AGENTIC_SYSCALL_WINDOW_S") {
        config.tools.window_s = value.max(1);
    }
    if let Some(value) = env_usize_opt("AGENTIC_SYSCALL_ERROR_BURST_KILL") {
        config.tools.error_burst_kill = value.max(1);
    }
    if let Some(value) = env_string("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS") {
        config.tools.remote_http_allowed_hosts = value
            .split(',')
            .map(|item| item.trim().to_ascii_lowercase())
            .filter(|item| !item.is_empty())
            .collect();
    }
    if let Some(value) = env_usize_opt("AGENTIC_REMOTE_TOOL_MAX_REQUEST_BYTES") {
        config.tools.remote_http_max_request_bytes = value.max(256);
    }
    if let Some(value) = env_usize_opt("AGENTIC_REMOTE_TOOL_MAX_RESPONSE_BYTES") {
        config.tools.remote_http_max_response_bytes = value.max(256);
    }
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
    std::env::var(name).ok().map(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

pub(crate) fn env_u16(name: &str) -> Option<u16> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok())
}

pub(crate) fn env_u64_opt(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

pub(crate) fn env_f64_opt(name: &str) -> Option<f64> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
}

pub(crate) fn env_usize_opt(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
}

pub(crate) fn env_string(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
}
