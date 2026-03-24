use super::parser::repository_path;
/// Configuration models and structures.
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Default)]
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
                "workflow_control_api_v1".to_string(),
                "orchestration_list_v1".to_string(),
                "orchestration_status_v1".to_string(),
                "orchestration_control_v1".to_string(),
                "scheduled_job_list_v1".to_string(),
                "scheduled_job_control_v1".to_string(),
                "artifact_list_v1".to_string(),
                "workflow_definition_schema_v1".to_string(),
                "list_models_v1".to_string(),
                "model_info_v1".to_string(),
                "backend_diag_v1".to_string(),
                "list_tools_v1".to_string(),
                "tool_registry_v1".to_string(),
                "tool_info_v1".to_string(),
                "tool_remote_v1".to_string(),
                "tool_alias_compat_v1".to_string(),
                "orchestrate_v1".to_string(),
                "schedule_job_v1".to_string(),
                "retry_task_v1".to_string(),
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
            poll_timeout_ms: 500,
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
    pub retrieve_candidate_limit: usize,
    pub retrieve_max_segment_chars: usize,
    pub retrieve_min_score: f64,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            default_strategy: "sliding".to_string(),
            default_window_tokens: 2048,
            compaction_trigger_tokens: 1792,
            compaction_target_tokens: 1536,
            retrieve_top_k: 3,
            retrieve_candidate_limit: 64,
            retrieve_max_segment_chars: 768,
            retrieve_min_score: 0.12,
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
    pub executable: String,
    pub port_base: u16,
    pub startup_timeout_ms: u64,
    pub health_poll_ms: u64,
    pub legacy_endpoint_override: String,
    pub timeout_ms: u64,
    pub chunk_tokens: usize,
}

impl Default for ExternalLlamaCppConfig {
    fn default() -> Self {
        Self {
            executable: "llama-server".to_string(),
            port_base: 8080,
            startup_timeout_ms: 120_000,
            health_poll_ms: 250,
            legacy_endpoint_override: String::new(),
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
