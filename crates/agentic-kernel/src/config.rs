use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use toml::Value as TomlValue;

static KERNEL_CONFIG: OnceLock<KernelConfig> = OnceLock::new();

#[derive(Debug, Clone)]
struct ConfigBootstrapPaths {
    config_files: Vec<PathBuf>,
    env_file: PathBuf,
}

impl ConfigBootstrapPaths {
    fn primary_config_path(&self) -> PathBuf {
        self.config_files
            .first()
            .cloned()
            .unwrap_or_else(|| repository_path("config/kernel/base.toml"))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct KernelConfig {
    pub network: NetworkConfig,
    pub protocol: ProtocolRuntimeConfig,
    pub paths: PathsConfig,
    pub memory: MemoryRuntimeConfig,
    pub resources: ResourceGovernorConfig,
    pub context: ContextConfig,
    pub checkpoint: CheckpointConfig,
    pub auth: AuthConfig,
    pub external_llamacpp: ExternalLlamaCppConfig,
    pub openai_responses: OpenAIResponsesConfig,
    pub groq_responses: GroqResponsesConfig,
    pub openrouter: OpenRouterConfig,
    pub exec: ExecConfig,
    pub orchestrator: OrchestratorConfig,
    pub tools: ToolsRuntimeConfig,
    pub generation: GenerationProfilesConfig,
    pub scheduler: SchedulerConfig,
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            network: NetworkConfig::default(),
            protocol: ProtocolRuntimeConfig::default(),
            paths: PathsConfig::default(),
            memory: MemoryRuntimeConfig::default(),
            resources: ResourceGovernorConfig::default(),
            context: ContextConfig::default(),
            checkpoint: CheckpointConfig::default(),
            auth: AuthConfig::default(),
            external_llamacpp: ExternalLlamaCppConfig::default(),
            openai_responses: OpenAIResponsesConfig::default(),
            groq_responses: GroqResponsesConfig::default(),
            openrouter: OpenRouterConfig::default(),
            exec: ExecConfig::default(),
            orchestrator: OrchestratorConfig::default(),
            tools: ToolsRuntimeConfig::default(),
            generation: GenerationProfilesConfig::default(),
            scheduler: SchedulerConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProtocolRuntimeConfig {
    pub default_contract_v1: bool,
    pub allow_legacy_fallback: bool,
    pub enabled_capabilities: Vec<String>,
}

impl Default for ProtocolRuntimeConfig {
    fn default() -> Self {
        Self {
            default_contract_v1: false,
            allow_legacy_fallback: true,
            enabled_capabilities: vec![
                "control_envelope_v1".to_string(),
                "hello_v1".to_string(),
                "status_v1".to_string(),
                "pid_status_v1".to_string(),
                "orch_status_v1".to_string(),
                "list_models_v1".to_string(),
                "model_info_v1".to_string(),
                "backend_diag_v1".to_string(),
                "list_tools_v1".to_string(),
                "tool_registry_v1".to_string(),
                "tool_info_v1".to_string(),
                "tool_remote_v1".to_string(),
                "tool_alias_compat_v1".to_string(),
                "orchestrate_v1".to_string(),
                "event_stream_v1".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub host: String,
    pub port: u16,
    pub poll_timeout_ms: u64,
    pub log_connections: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 6380,
            poll_timeout_ms: 5,
            log_connections: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PathsConfig {
    pub models_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub database_path: PathBuf,
    pub checkpoint_path: PathBuf,
    pub kernel_token_path: PathBuf,
    pub remote_provider_catalog_path: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            models_dir: repository_path("models"),
            workspace_dir: repository_path("workspace"),
            database_path: repository_path("workspace/agenticos.db"),
            checkpoint_path: repository_path("workspace/checkpoint.json"),
            kernel_token_path: repository_path("workspace/.kernel_token"),
            remote_provider_catalog_path: repository_path("config/providers/remote_providers.toml"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MemoryRuntimeConfig {
    pub swap_async: bool,
    pub swap_dir: PathBuf,
    pub token_slot_quota_per_pid: usize,
}

impl Default for MemoryRuntimeConfig {
    fn default() -> Self {
        Self {
            swap_async: true,
            swap_dir: repository_path("workspace/swap"),
            token_slot_quota_per_pid: 4096,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ResourceGovernorConfig {
    pub ram_budget_bytes: u64,
    pub vram_budget_bytes: u64,
    pub min_ram_headroom_bytes: u64,
    pub min_vram_headroom_bytes: u64,
    pub local_runtime_ram_scale: f64,
    pub local_runtime_vram_scale: f64,
    pub local_runtime_ram_overhead_bytes: u64,
    pub local_runtime_vram_overhead_bytes: u64,
    pub max_queue_entries: usize,
}

impl Default for ResourceGovernorConfig {
    fn default() -> Self {
        Self {
            ram_budget_bytes: 0,
            vram_budget_bytes: 0,
            min_ram_headroom_bytes: 0,
            min_vram_headroom_bytes: 0,
            local_runtime_ram_scale: 1.15,
            local_runtime_vram_scale: 1.05,
            local_runtime_ram_overhead_bytes: 268_435_456,
            local_runtime_vram_overhead_bytes: 134_217_728,
            max_queue_entries: 32,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ContextConfig {
    pub default_strategy: String,
    pub default_window_tokens: usize,
    pub compaction_trigger_tokens: usize,
    pub compaction_target_tokens: usize,
    pub retrieve_top_k: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            default_strategy: "sliding".to_string(),
            default_window_tokens: 2048,
            compaction_trigger_tokens: 1792,
            compaction_target_tokens: 1536,
            retrieve_top_k: 3,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CheckpointConfig {
    pub interval_secs: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct AuthConfig {
    pub disabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ExternalLlamaCppConfig {
    pub endpoint: String,
    pub timeout_ms: u64,
    pub chunk_tokens: usize,
}

impl Default for ExternalLlamaCppConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            timeout_ms: 300_000,
            chunk_tokens: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemoteProviderRuntimeConfig {
    pub backend_id: String,
    pub adapter_kind: RemoteAdapterKind,
    pub endpoint: String,
    pub api_key: String,
    pub default_model: String,
    pub timeout_ms: u64,
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
    pub stream: bool,
    #[allow(dead_code)]
    pub tokenizer_path: Option<PathBuf>,
    pub input_price_usd_per_mtok: f64,
    pub output_price_usd_per_mtok: f64,
    pub http_referer: String,
    pub app_title: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAdapterKind {
    #[default]
    #[serde(rename = "openai_compatible")]
    OpenAICompatible,
}

impl RemoteAdapterKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAICompatible => "openai_compatible",
        }
    }
}

macro_rules! define_remote_openai_config {
    ($name:ident, $backend_id:expr, $endpoint:expr) => {
        #[derive(Debug, Clone, Deserialize)]
        #[serde(default)]
        pub struct $name {
            pub endpoint: String,
            pub api_key: String,
            pub default_model: String,
            pub timeout_ms: u64,
            pub max_request_bytes: usize,
            pub max_response_bytes: usize,
            pub stream: bool,
            pub tokenizer_path: Option<PathBuf>,
            pub input_price_usd_per_mtok: f64,
            pub output_price_usd_per_mtok: f64,
            pub http_referer: String,
            pub app_title: String,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    endpoint: $endpoint.to_string(),
                    api_key: String::new(),
                    default_model: String::new(),
                    timeout_ms: 120_000,
                    max_request_bytes: 512 * 1024,
                    max_response_bytes: 4 * 1024 * 1024,
                    stream: true,
                    tokenizer_path: None,
                    input_price_usd_per_mtok: 0.0,
                    output_price_usd_per_mtok: 0.0,
                    http_referer: String::new(),
                    app_title: String::new(),
                }
            }
        }

        impl From<$name> for RemoteProviderRuntimeConfig {
            fn from(value: $name) -> Self {
                Self {
                    backend_id: $backend_id.to_string(),
                    adapter_kind: RemoteAdapterKind::OpenAICompatible,
                    endpoint: value.endpoint,
                    api_key: value.api_key,
                    default_model: value.default_model,
                    timeout_ms: value.timeout_ms,
                    max_request_bytes: value.max_request_bytes,
                    max_response_bytes: value.max_response_bytes,
                    stream: value.stream,
                    tokenizer_path: value.tokenizer_path,
                    input_price_usd_per_mtok: value.input_price_usd_per_mtok,
                    output_price_usd_per_mtok: value.output_price_usd_per_mtok,
                    http_referer: value.http_referer,
                    app_title: value.app_title,
                }
            }
        }
    };
}

define_remote_openai_config!(
    OpenAIResponsesConfig,
    "openai-responses",
    "https://api.openai.com/v1"
);
define_remote_openai_config!(
    GroqResponsesConfig,
    "groq-responses",
    "https://api.groq.com/openai/v1"
);
define_remote_openai_config!(
    OpenRouterConfig,
    "openrouter",
    "https://openrouter.ai/api/v1"
);

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ExecConfig {
    pub auto_switch: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OrchestratorConfig {
    pub max_output_chars: usize,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_output_chars: 4096,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ToolsRuntimeConfig {
    pub sandbox_mode: String,
    pub allow_host_fallback: bool,
    pub timeout_s: u64,
    pub max_calls_per_window: usize,
    pub window_s: u64,
    pub error_burst_kill: usize,
    pub output_truncate_len: usize,
    pub remote_http_allowed_hosts: Vec<String>,
    pub remote_http_max_request_bytes: usize,
    pub remote_http_max_response_bytes: usize,
    pub audit_log_file: String,
    pub temp_script_prefix: String,
}

impl Default for ToolsRuntimeConfig {
    fn default() -> Self {
        Self {
            sandbox_mode: "host".to_string(),
            allow_host_fallback: true,
            timeout_s: 8,
            max_calls_per_window: 12,
            window_s: 10,
            error_burst_kill: 3,
            output_truncate_len: 2000,
            remote_http_allowed_hosts: Vec::new(),
            remote_http_max_request_bytes: 16 * 1024,
            remote_http_max_response_bytes: 64 * 1024,
            audit_log_file: "syscall_audit.log".to_string(),
            temp_script_prefix: "agent_script_".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GenerationProfilesConfig {
    pub llama: GenerationProfile,
    pub qwen: GenerationProfile,
    pub mistral: GenerationProfile,
    pub unknown: GenerationProfile,
}

impl Default for GenerationProfilesConfig {
    fn default() -> Self {
        Self {
            llama: GenerationProfile::llama_default(),
            qwen: GenerationProfile::qwen_default(),
            mistral: GenerationProfile::mistral_default(),
            unknown: GenerationProfile::unknown_default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GenerationProfile {
    pub temperature: f64,
    pub top_p: f64,
    pub seed: u64,
    pub max_tokens: usize,
}

impl GenerationProfile {
    fn llama_default() -> Self {
        Self {
            temperature: 0.7,
            top_p: 0.9,
            seed: 299_792_458,
            max_tokens: 500,
        }
    }

    fn qwen_default() -> Self {
        Self::llama_default()
    }

    fn mistral_default() -> Self {
        Self {
            temperature: 0.7,
            top_p: 0.92,
            seed: 299_792_458,
            max_tokens: 500,
        }
    }

    fn unknown_default() -> Self {
        Self::llama_default()
    }
}

impl Default for GenerationProfile {
    fn default() -> Self {
        Self::llama_default()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SchedulerConfig {
    pub fast: SchedulerQuotaConfig,
    pub code: SchedulerQuotaConfig,
    pub reasoning: SchedulerQuotaConfig,
    pub general: SchedulerQuotaConfig,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            fast: SchedulerQuotaConfig {
                max_tokens: 512,
                max_syscalls: 2,
            },
            code: SchedulerQuotaConfig {
                max_tokens: 4096,
                max_syscalls: 16,
            },
            reasoning: SchedulerQuotaConfig {
                max_tokens: 8192,
                max_syscalls: 8,
            },
            general: SchedulerQuotaConfig {
                max_tokens: 2048,
                max_syscalls: 8,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct SchedulerQuotaConfig {
    pub max_tokens: usize,
    pub max_syscalls: usize,
}

impl Default for SchedulerQuotaConfig {
    fn default() -> Self {
        Self {
            max_tokens: 2048,
            max_syscalls: 8,
        }
    }
}

pub fn initialize() -> Result<&'static KernelConfig, String> {
    if let Some(config) = KERNEL_CONFIG.get() {
        return Ok(config);
    }

    let config = load_kernel_config()?;
    let _ = KERNEL_CONFIG.set(config);
    Ok(KERNEL_CONFIG.get().expect("kernel config initialized"))
}

pub fn kernel_config() -> &'static KernelConfig {
    KERNEL_CONFIG.get_or_init(|| {
        load_kernel_config().unwrap_or_else(|err| {
            eprintln!("AgenticOS config warning: {err}. Falling back to built-in defaults.");
            KernelConfig::default()
        })
    })
}

#[allow(dead_code)]
pub fn config_file_path() -> PathBuf {
    resolve_config_bootstrap_paths().primary_config_path()
}

fn load_kernel_config() -> Result<KernelConfig, String> {
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

fn resolve_config_bootstrap_paths() -> ConfigBootstrapPaths {
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

fn load_merged_toml_config(config_files: &[PathBuf]) -> Result<Option<TomlValue>, String> {
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

fn merge_toml_value(base: &mut TomlValue, overlay: TomlValue) {
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

fn load_env_file(path: &Path) -> Result<(), String> {
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

fn parse_env_value(value: &str) -> String {
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

fn normalize_config_paths(config: &mut KernelConfig, config_path: &Path) {
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

fn absolutize_from(base_dir: &Path, path: &mut PathBuf) {
    if path.is_relative() {
        *path = base_dir.join(&*path);
    }
}

fn absolutize_remote_tokenizer_path(base_dir: &Path, path: &mut Option<PathBuf>) {
    if let Some(path) = path.as_mut() {
        absolutize_from(base_dir, path);
    }
}

fn apply_env_overrides(config: &mut KernelConfig) {
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
    if let Some(value) = env_u64_opt("AGENTIC_CHECKPOINT_INTERVAL_SECS") {
        config.checkpoint.interval_secs = value;
    }
    if let Some(value) = env_bool_opt("AGENTIC_AUTH_DISABLED") {
        config.auth.disabled = value;
    }
    if let Some(value) = env_string("AGENTIC_LLAMACPP_ENDPOINT") {
        config.external_llamacpp.endpoint = value;
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

fn env_bool_opt(name: &str) -> Option<bool> {
    std::env::var(name).ok().map(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn env_u16(name: &str) -> Option<u16> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok())
}

fn env_u64_opt(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

fn env_f64_opt(name: &str) -> Option<f64> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
}

fn env_usize_opt(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
}

fn env_string(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
}
