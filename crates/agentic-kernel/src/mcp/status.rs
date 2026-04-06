#[derive(Debug, Clone, Default)]
pub(crate) struct McpBridgeStatusSnapshot {
    pub(crate) servers: Vec<McpServerStatusSnapshot>,
}

#[derive(Debug, Clone)]
pub(crate) struct McpServerStatusSnapshot {
    pub(crate) server_id: String,
    pub(crate) label: Option<String>,
    pub(crate) transport: String,
    pub(crate) trust_level: String,
    pub(crate) auth_mode: String,
    pub(crate) health: String,
    pub(crate) tool_prefix: String,
    pub(crate) enabled: bool,
    pub(crate) connected: bool,
    pub(crate) default_allowlisted: bool,
    pub(crate) approval_required: bool,
    pub(crate) roots_enabled: bool,
    pub(crate) exposed_tools: Vec<String>,
    pub(crate) discovered_tools: Vec<McpDiscoveredToolSnapshot>,
    pub(crate) prompts: Vec<McpPromptStatusSnapshot>,
    pub(crate) resources: Vec<McpResourceStatusSnapshot>,
    pub(crate) last_latency_ms: Option<u64>,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct McpDiscoveredToolSnapshot {
    pub(crate) agentic_tool_name: String,
    pub(crate) target_name: String,
    pub(crate) title: Option<String>,
    pub(crate) description: String,
    pub(crate) dangerous: bool,
    pub(crate) default_allowlisted: bool,
    pub(crate) approval_required: bool,
    pub(crate) read_only_hint: bool,
    pub(crate) destructive_hint: bool,
    pub(crate) idempotent_hint: bool,
    pub(crate) open_world_hint: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct McpPromptStatusSnapshot {
    pub(crate) name: String,
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct McpResourceStatusSnapshot {
    pub(crate) name: String,
    pub(crate) title: Option<String>,
    pub(crate) uri: String,
    pub(crate) description: Option<String>,
    pub(crate) mime_type: Option<String>,
}
