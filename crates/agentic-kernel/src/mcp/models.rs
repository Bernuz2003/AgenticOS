use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct McpImplementation {
    pub(crate) name: String,
    pub(crate) version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct McpServerCapabilities {
    pub(crate) tools_list_changed: bool,
    pub(crate) resources_list_changed: bool,
    pub(crate) resources_subscribe: bool,
    pub(crate) prompts_list_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct McpInitializeResult {
    pub(crate) protocol_version: String,
    pub(crate) capabilities: McpServerCapabilities,
    pub(crate) server_info: McpImplementation,
    pub(crate) instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct McpToolAnnotations {
    pub(crate) title: Option<String>,
    pub(crate) read_only_hint: bool,
    pub(crate) destructive_hint: bool,
    pub(crate) idempotent_hint: bool,
    pub(crate) open_world_hint: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct McpToolDefinition {
    pub(crate) name: String,
    pub(crate) title: Option<String>,
    pub(crate) description: String,
    pub(crate) input_schema: Value,
    pub(crate) output_schema: Option<Value>,
    pub(crate) annotations: McpToolAnnotations,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct McpPromptDefinition {
    pub(crate) name: String,
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct McpResourceDefinition {
    pub(crate) name: String,
    pub(crate) title: Option<String>,
    pub(crate) uri: String,
    pub(crate) description: Option<String>,
    pub(crate) mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct McpRoot {
    pub(crate) uri: String,
    pub(crate) name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct McpCallToolResult {
    pub(crate) content: Vec<Value>,
    pub(crate) structured_content: Option<Value>,
    pub(crate) is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct McpInvocationMetadata {
    pub(crate) provider: String,
    pub(crate) server_id: String,
    pub(crate) server_label: Option<String>,
    pub(crate) transport: String,
    pub(crate) target_name: String,
    pub(crate) trust_level: String,
    pub(crate) auth_mode: String,
    pub(crate) latency_ms: u128,
    pub(crate) approval_required: bool,
    pub(crate) validation_attempted: bool,
    pub(crate) validation_passed: bool,
    pub(crate) trust_filters: Vec<String>,
    pub(crate) roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct McpBridgeToolResponse {
    pub(crate) output: String,
    pub(crate) content: Vec<Value>,
    pub(crate) structured_content: Option<Value>,
    pub(crate) is_error: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub(crate) mcp: McpInvocationMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct McpBridgeErrorBody {
    pub(crate) kind: String,
    pub(crate) message: String,
    pub(crate) mcp: Option<McpInvocationMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct McpBridgeErrorResponse {
    pub(crate) error: McpBridgeErrorBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct McpBridgeInvocationRequest {
    pub(crate) input: Value,
    pub(crate) context: crate::tools::invocation::ToolContext,
}
