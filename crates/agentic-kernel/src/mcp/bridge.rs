use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use serde_json::{json, Value};

use crate::config::{KernelConfig, McpServerConfig, McpTransportConfig, McpTrustLevel};
use crate::mcp::client::McpStdioClient;
use crate::mcp::http_bridge;
use crate::mcp::models::{
    McpBridgeErrorBody, McpBridgeErrorResponse, McpBridgeInvocationRequest, McpBridgeToolResponse,
    McpCallToolResult, McpInitializeResult, McpInvocationMetadata, McpPromptDefinition,
    McpResourceDefinition, McpToolAnnotations, McpToolDefinition,
};
use crate::mcp::roots::roots_for_context;
use crate::mcp::status::{
    McpBridgeStatusSnapshot, McpDiscoveredToolSnapshot, McpPromptStatusSnapshot,
    McpResourceStatusSnapshot, McpServerStatusSnapshot,
};
use crate::tool_registry::{
    ToolBackendConfig, ToolBackendKind, ToolInteropDescriptor, ToolInteropHints, ToolRegistry,
    ToolRegistryEntry, ToolSource,
};
use crate::tools::invocation::{normalize_tool_name, ToolCaller};
use crate::tools::schema::{ensure_valid_schema, validate_value};

#[derive(Debug)]
pub(crate) struct McpBridgeRuntime {
    state: Arc<Mutex<McpBridgeState>>,
    shutdown_tx: Option<std::sync::mpsc::Sender<()>>,
    server_handle: Option<JoinHandle<()>>,
    registered_tool_names: Vec<String>,
}

impl McpBridgeRuntime {
    pub(crate) fn start(
        config: &KernelConfig,
        tool_registry: &mut ToolRegistry,
    ) -> Result<Option<Self>, String> {
        if !config.mcp.enabled {
            return Ok(None);
        }

        let enabled_servers: Vec<McpServerConfig> = config
            .mcp
            .servers
            .iter()
            .filter(|server| server.enabled)
            .cloned()
            .collect();
        if enabled_servers.is_empty() {
            return Ok(None);
        }

        let state = Arc::new(Mutex::new(McpBridgeState::new(enabled_servers)));
        let token = random_token()?;
        let (listen_addr, shutdown_tx, server_handle) = http_bridge::spawn(
            &config.mcp.bridge_host,
            config.mcp.bridge_port,
            &config.mcp.bridge_token_header,
            &token,
            Arc::clone(&state),
        )?;

        let base_url = format!("http://{}:{}", listen_addr.ip(), listen_addr.port());
        let registered_tool_names = {
            let mut guard = state
                .lock()
                .map_err(|_| "MCP bridge state lock poisoned during startup.".to_string())?;
            guard.sync_registry(
                tool_registry,
                &base_url,
                &config.mcp.bridge_token_header,
                &token,
            )
        };

        Ok(Some(Self {
            state,
            shutdown_tx: Some(shutdown_tx),
            server_handle: Some(server_handle),
            registered_tool_names,
        }))
    }

    pub(crate) fn shutdown(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        if let Some(handle) = self.server_handle.take() {
            let _ = handle.join();
        }
    }

    pub(crate) fn registered_tool_names(&self) -> &[String] {
        &self.registered_tool_names
    }

    pub(crate) fn status_snapshot(&self) -> Option<McpBridgeStatusSnapshot> {
        self.state
            .lock()
            .ok()
            .map(|state| state.status_snapshot())
    }
}

impl Drop for McpBridgeRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Debug)]
pub(super) struct BridgeInvocationError {
    pub(super) status_code: u16,
    pub(super) body: McpBridgeErrorResponse,
}

#[derive(Debug)]
pub(super) struct McpBridgeState {
    servers: HashMap<String, McpServerSession>,
    tools: BTreeMap<String, RegisteredMcpTool>,
}

impl McpBridgeState {
    fn new(servers: Vec<McpServerConfig>) -> Self {
        Self {
            servers: servers
                .into_iter()
                .map(|server| (server.id.clone(), McpServerSession::new(server)))
                .collect(),
            tools: BTreeMap::new(),
        }
    }

    fn sync_registry(
        &mut self,
        tool_registry: &mut ToolRegistry,
        base_url: &str,
        bridge_token_header: &str,
        bridge_token: &str,
    ) -> Vec<String> {
        let mut registered = Vec::new();
        let mut discovered_tools = BTreeMap::new();
        let mut registered_tools = BTreeMap::new();

        for server in self.servers.values_mut() {
            if let Err(err) = server.sync() {
                tracing::warn!(server_id = server.config.id, %err, "MCP discovery failed");
                continue;
            }

            for tool in server.selected_tools() {
                if let Err(err) = validate_discovered_tool(tool) {
                    tracing::warn!(
                        server_id = server.config.id,
                        tool_name = tool.name,
                        %err,
                        "Skipping MCP tool with invalid discovery schema"
                    );
                    continue;
                }
                let Some(agentic_name) = build_agentic_tool_name(&server.config, tool) else {
                    tracing::warn!(
                        server_id = server.config.id,
                        tool_name = tool.name,
                        "Skipping MCP tool with invalid normalized name"
                    );
                    continue;
                };
                discovered_tools.insert(
                    agentic_name.clone(),
                    RegisteredMcpTool::from_session(server, tool, agentic_name),
                );
            }
        }

        for tool in discovered_tools.values() {
            match tool_registry.register(tool.registry_entry(
                base_url,
                bridge_token_header,
                bridge_token,
            )) {
                Ok(()) => {
                    registered.push(tool.agentic_name.clone());
                    registered_tools.insert(tool.agentic_name.clone(), tool.clone());
                }
                Err(err) => {
                    tracing::warn!(
                        tool_name = tool.agentic_name,
                        server_id = tool.server_id,
                        %err,
                        "Failed to register MCP-backed tool"
                    );
                }
            }
        }

        self.tools = registered_tools;
        registered
    }

    fn status_snapshot(&self) -> McpBridgeStatusSnapshot {
        McpBridgeStatusSnapshot {
            servers: self
                .servers
                .values()
                .map(|server| server.status_snapshot(self.tools.values()))
                .collect(),
        }
    }

    pub(super) fn invoke_tool(
        &mut self,
        agentic_tool_name: &str,
        request: McpBridgeInvocationRequest,
    ) -> Result<McpBridgeToolResponse, BridgeInvocationError> {
        let tool =
            self.tools
                .get(agentic_tool_name)
                .cloned()
                .ok_or_else(|| BridgeInvocationError {
                    status_code: 404,
                    body: error_response(
                        "mcp_tool_not_registered",
                        "MCP-backed tool is not registered.",
                        None,
                    ),
                })?;

        let Some(server) = self.servers.get_mut(&tool.server_id) else {
            return Err(BridgeInvocationError {
                status_code: 404,
                body: error_response(
                    "mcp_server_not_found",
                    "Configured MCP server is missing.",
                    None,
                ),
            });
        };

        if !request.context.permissions.allows_tool(agentic_tool_name) {
            return Err(BridgeInvocationError {
                status_code: 403,
                body: error_response(
                    "policy_denied",
                    "Process policy denies this MCP-backed tool.",
                    None,
                ),
            });
        }

        let roots = if server.config.roots_enabled {
            roots_for_context(&request.context).map_err(|err| BridgeInvocationError {
                status_code: 403,
                body: error_response("roots_denied", &err, None),
            })?
        } else {
            Vec::new()
        };

        let started_at = Instant::now();
        let call_result = server
            .call_tool(&tool.target_name, request.input, roots.clone())
            .map_err(|err| {
                let metadata = Some(base_metadata(
                    &tool,
                    &roots,
                    started_at.elapsed().as_millis(),
                    false,
                    false,
                    vec![],
                ));
                BridgeInvocationError {
                    status_code: if err.contains("timed out") { 504 } else { 502 },
                    body: error_response("mcp_transport_failed", &err, metadata),
                }
            })?;
        let latency_ms = started_at.elapsed().as_millis();
        let mut trust_filters = Vec::new();
        let (validation_attempted, validation_passed) =
            validate_result(&tool, &call_result, &mut trust_filters).map_err(|err| {
                BridgeInvocationError {
                    status_code: 422,
                    body: error_response(
                        "mcp_result_validation_failed",
                        &err,
                        Some(base_metadata(
                            &tool,
                            &roots,
                            latency_ms,
                            true,
                            false,
                            trust_filters.clone(),
                        )),
                    ),
                }
            })?;

        let output = render_tool_output(&call_result);
        let metadata = base_metadata(
            &tool,
            &roots,
            latency_ms,
            validation_attempted,
            validation_passed,
            trust_filters.clone(),
        );

        if call_result.is_error {
            return Err(BridgeInvocationError {
                status_code: 500,
                body: error_response("mcp_tool_execution_failed", &output, Some(metadata)),
            });
        }

        server.last_latency_ms = Some(latency_ms);
        server.last_error = None;

        Ok(McpBridgeToolResponse {
            output,
            content: call_result.content,
            structured_content: call_result.structured_content,
            is_error: false,
            warnings: trust_filters
                .iter()
                .map(|filter| format!("MCP trust filter applied: {filter}"))
                .collect(),
            mcp: metadata,
        })
    }
}

#[derive(Debug)]
struct McpServerSession {
    config: McpServerConfig,
    client: Option<McpStdioClient>,
    initialize: Option<McpInitializeResult>,
    tools: Vec<McpToolDefinition>,
    #[allow(dead_code)]
    prompts: Vec<McpPromptDefinition>,
    #[allow(dead_code)]
    resources: Vec<McpResourceDefinition>,
    last_latency_ms: Option<u128>,
    last_error: Option<String>,
}

impl McpServerSession {
    fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            client: None,
            initialize: None,
            tools: Vec::new(),
            prompts: Vec::new(),
            resources: Vec::new(),
            last_latency_ms: None,
            last_error: None,
        }
    }

    fn sync(&mut self) -> Result<(), String> {
        let result: Result<(), String> = (|| {
            let tools = {
                let client = self.ensure_client()?;
                client.list_tools()?
            };
            self.tools = tools;

            let capabilities = self
                .initialize
                .as_ref()
                .map(|initialize| initialize.capabilities.clone());

            if let Some(capabilities) = capabilities {
                let prompts = {
                    let client = self.ensure_client()?;
                    client.list_prompts(&capabilities).unwrap_or_default()
                };
                let resources = {
                    let client = self.ensure_client()?;
                    client.list_resources(&capabilities).unwrap_or_default()
                };
                self.prompts = prompts;
                self.resources = resources;
            }
            Ok(())
        })();

        match result {
            Ok(()) => {
                self.last_error = None;
                Ok(())
            }
            Err(err) => {
                self.record_transport_failure(&err);
                Err(err)
            }
        }
    }

    fn selected_tools(&self) -> impl Iterator<Item = &McpToolDefinition> {
        let exposed = self
            .config
            .exposed_tools
            .iter()
            .filter_map(|tool_name| normalize_tool_name(tool_name).ok())
            .collect::<Vec<_>>();

        self.tools.iter().filter(move |tool| {
            exposed.is_empty()
                || normalize_tool_name(&tool.name)
                    .ok()
                    .is_some_and(|tool_name| exposed.contains(&tool_name))
        })
    }

    fn call_tool(
        &mut self,
        tool_name: &str,
        input: Value,
        roots: Vec<crate::mcp::models::McpRoot>,
    ) -> Result<McpCallToolResult, String> {
        let result = self.ensure_client()?.call_tool(tool_name, input, roots);
        match result {
            Ok(value) => {
                self.last_error = None;
                Ok(value)
            }
            Err(err) => {
                self.record_transport_failure(&err);
                Err(err)
            }
        }
    }

    fn ensure_client(&mut self) -> Result<&mut McpStdioClient, String> {
        if self.client.is_none() {
            let mut client = McpStdioClient::connect(&self.config)?;
            let initialize = client.initialize()?;
            self.initialize = Some(initialize);
            self.client = Some(client);
        }

        self.client
            .as_mut()
            .ok_or_else(|| format!("MCP client '{}' is unavailable.", self.config.id))
    }

    fn record_transport_failure(&mut self, err: &str) {
        self.last_error = Some(err.to_string());
        if should_reset_client(err) {
            self.client = None;
            self.initialize = None;
        }
    }

    fn status_snapshot<'a>(
        &self,
        registered_tools: impl Iterator<Item = &'a RegisteredMcpTool>,
    ) -> McpServerStatusSnapshot {
        let discovered_tools = registered_tools
            .filter(|tool| tool.server_id == self.config.id)
            .map(|tool| McpDiscoveredToolSnapshot {
                agentic_tool_name: tool.agentic_name.clone(),
                target_name: tool.target_name.clone(),
                title: tool.hints.title.clone(),
                description: tool.description.clone(),
                dangerous: tool.dangerous,
                default_allowlisted: tool.default_allowlisted,
                approval_required: tool.approval_required,
                read_only_hint: tool.hints.read_only_hint,
                destructive_hint: tool.hints.destructive_hint,
                idempotent_hint: tool.hints.idempotent_hint,
                open_world_hint: tool.hints.open_world_hint,
            })
            .collect();

        McpServerStatusSnapshot {
            server_id: self.config.id.clone(),
            label: self.config.label.clone(),
            transport: self.config.transport.kind().to_string(),
            trust_level: self.config.trust_level.as_str().to_string(),
            auth_mode: auth_mode_for_transport(&self.config.transport),
            health: server_health(self),
            tool_prefix: self
                .config
                .tool_prefix
                .clone()
                .unwrap_or_else(|| self.config.id.clone()),
            enabled: self.config.enabled,
            connected: self.client.is_some() && self.initialize.is_some(),
            default_allowlisted: self.config.default_allowlisted,
            approval_required: self.config.approval_required,
            roots_enabled: self.config.roots_enabled,
            exposed_tools: self.config.exposed_tools.clone(),
            discovered_tools,
            prompts: self
                .prompts
                .iter()
                .map(|prompt| McpPromptStatusSnapshot {
                    name: prompt.name.clone(),
                    title: prompt.title.clone(),
                    description: prompt.description.clone(),
                })
                .collect(),
            resources: self
                .resources
                .iter()
                .map(|resource| McpResourceStatusSnapshot {
                    name: resource.name.clone(),
                    title: resource.title.clone(),
                    uri: resource.uri.clone(),
                    description: resource.description.clone(),
                    mime_type: resource.mime_type.clone(),
                })
                .collect(),
            last_latency_ms: self
                .last_latency_ms
                .map(|latency| latency.min(u64::MAX as u128) as u64),
            last_error: self.last_error.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct RegisteredMcpTool {
    agentic_name: String,
    server_id: String,
    server_label: Option<String>,
    transport: String,
    timeout_ms: u64,
    target_name: String,
    trust_level: String,
    auth_mode: String,
    default_allowlisted: bool,
    approval_required: bool,
    dangerous: bool,
    hints: McpToolAnnotations,
    description: String,
    input_schema: Value,
    output_schema: Option<Value>,
}

impl RegisteredMcpTool {
    fn from_session(
        session: &McpServerSession,
        tool: &McpToolDefinition,
        agentic_name: String,
    ) -> Self {
        let dangerous = match session.config.trust_level {
            McpTrustLevel::Trusted => {
                !tool.annotations.read_only_hint
                    || tool.annotations.destructive_hint
                    || tool.annotations.open_world_hint
            }
            McpTrustLevel::Untrusted => true,
        };

        Self {
            agentic_name,
            server_id: session.config.id.clone(),
            server_label: session.config.label.clone(),
            transport: session.config.transport.kind().to_string(),
            timeout_ms: transport_timeout_ms(&session.config.transport),
            target_name: tool.name.clone(),
            trust_level: session.config.trust_level.as_str().to_string(),
            auth_mode: auth_mode_for_transport(&session.config.transport),
            default_allowlisted: session.config.default_allowlisted,
            approval_required: session.config.approval_required,
            dangerous,
            hints: tool.annotations.clone(),
            description: if tool.description.trim().is_empty() {
                format!(
                    "MCP tool '{}' from server '{}'.",
                    tool.name, session.config.id
                )
            } else {
                tool.description.clone()
            },
            input_schema: tool.input_schema.clone(),
            output_schema: tool.output_schema.clone(),
        }
    }

    fn registry_entry(
        &self,
        base_url: &str,
        bridge_token_header: &str,
        bridge_token: &str,
    ) -> ToolRegistryEntry {
        let mut headers = BTreeMap::new();
        headers.insert(bridge_token_header.to_string(), bridge_token.to_string());

        ToolRegistryEntry {
            descriptor: crate::tool_registry::ToolDescriptor {
                name: self.agentic_name.clone(),
                aliases: Vec::new(),
                description: self.description.clone(),
                input_schema: self.input_schema.clone(),
                input_example: None,
                output_schema: bridge_output_schema(),
                allowed_callers: vec![
                    ToolCaller::AgentText,
                    ToolCaller::AgentSupervisor,
                    ToolCaller::Programmatic,
                ],
                backend_kind: ToolBackendKind::RemoteHttp,
                capabilities: vec!["mcp".to_string(), format!("mcp:{}", self.server_id)],
                dangerous: self.dangerous,
                enabled: true,
                default_allowlisted: self.default_allowlisted,
                approval_required: self.approval_required,
                interop: Some(ToolInteropDescriptor {
                    provider: "mcp".to_string(),
                    server_id: self.server_id.clone(),
                    server_label: self.server_label.clone(),
                    transport: self.transport.clone(),
                    target_name: self.target_name.clone(),
                    trust_level: self.trust_level.clone(),
                    auth_mode: self.auth_mode.clone(),
                    default_allowlisted: self.default_allowlisted,
                    approval_required: self.approval_required,
                    hints: ToolInteropHints {
                        title: self.hints.title.clone(),
                        read_only_hint: self.hints.read_only_hint,
                        destructive_hint: self.hints.destructive_hint,
                        idempotent_hint: self.hints.idempotent_hint,
                        open_world_hint: self.hints.open_world_hint,
                    },
                }),
                source: ToolSource::Runtime,
            },
            backend: ToolBackendConfig::RemoteHttp {
                url: format!("{base_url}/mcp/tools/{}", self.agentic_name),
                method: "POST".to_string(),
                timeout_ms: self.timeout_ms,
                headers: headers.into_iter().collect(),
            },
        }
    }
}

fn build_agentic_tool_name(config: &McpServerConfig, tool: &McpToolDefinition) -> Option<String> {
    let prefix = config.tool_prefix.as_deref().unwrap_or(&config.id);
    normalize_tool_name(&format!("{prefix}.{}", tool.name)).ok()
}

fn auth_mode_for_transport(transport: &McpTransportConfig) -> String {
    match transport {
        McpTransportConfig::Stdio { .. } => "environment".to_string(),
    }
}

fn transport_timeout_ms(transport: &McpTransportConfig) -> u64 {
    match transport {
        McpTransportConfig::Stdio { timeout_ms, .. } => (*timeout_ms).max(1),
    }
}

fn bridge_output_schema() -> Value {
    json!({
        "type": "object",
        "required": ["output", "content", "is_error", "mcp"],
        "properties": {
            "output": {"type": "string"},
            "content": {"type": "array"},
            "structured_content": {"type": "object"},
            "is_error": {"type": "boolean"},
            "warnings": {"type": "array", "items": {"type": "string"}},
            "mcp": {"type": "object"}
        },
        "additionalProperties": true
    })
}

fn render_tool_output(result: &McpCallToolResult) -> String {
    let mut lines = Vec::new();

    for block in &result.content {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        lines.push(text.to_string());
                    }
                }
            }
            Some("resource_link") => {
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("resource");
                let uri = block
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                lines.push(format!("Resource: {name} ({uri})"));
            }
            Some("resource") => {
                if let Some(resource) = block.get("resource") {
                    if let Some(text) = resource.get("text").and_then(Value::as_str) {
                        lines.push(text.to_string());
                    }
                }
            }
            Some("image") => lines.push("[image content omitted]".to_string()),
            Some("audio") => lines.push("[audio content omitted]".to_string()),
            _ => {}
        }
    }

    if lines.is_empty() {
        if let Some(structured) = result.structured_content.as_ref() {
            if let Ok(text) = serde_json::to_string_pretty(structured) {
                return text;
            }
        }
        return "MCP tool completed.".to_string();
    }

    lines.join("\n")
}

fn validate_discovered_tool(tool: &McpToolDefinition) -> Result<(), String> {
    ensure_valid_schema(
        &tool.input_schema,
        &format!("mcp tool '{}'.input_schema", tool.name),
    )?;
    if let Some(output_schema) = tool.output_schema.as_ref() {
        ensure_valid_schema(
            output_schema,
            &format!("mcp tool '{}'.output_schema", tool.name),
        )?;
    }
    Ok(())
}

fn validate_result(
    tool: &RegisteredMcpTool,
    result: &McpCallToolResult,
    trust_filters: &mut Vec<String>,
) -> Result<(bool, bool), String> {
    let Some(schema) = tool.output_schema.as_ref() else {
        return Ok((false, true));
    };
    if tool.trust_level != "trusted" {
        trust_filters.push("output_schema_ignored_untrusted_server".to_string());
        return Ok((false, true));
    }

    let Some(structured) = result.structured_content.as_ref() else {
        return Err(
            "MCP tool declared outputSchema but returned no structuredContent.".to_string(),
        );
    };

    validate_value(
        schema,
        structured,
        &format!("mcp tool '{}'.output_schema", tool.target_name),
    )
    .map_err(|err| format!("Structured MCP output failed validation: {err}"))?;
    Ok((true, true))
}

fn base_metadata(
    tool: &RegisteredMcpTool,
    roots: &[crate::mcp::models::McpRoot],
    latency_ms: u128,
    validation_attempted: bool,
    validation_passed: bool,
    trust_filters: Vec<String>,
) -> McpInvocationMetadata {
    McpInvocationMetadata {
        provider: "mcp".to_string(),
        server_id: tool.server_id.clone(),
        server_label: tool.server_label.clone(),
        transport: tool.transport.clone(),
        target_name: tool.target_name.clone(),
        trust_level: tool.trust_level.clone(),
        auth_mode: tool.auth_mode.clone(),
        latency_ms,
        approval_required: tool.approval_required,
        validation_attempted,
        validation_passed,
        trust_filters,
        roots: roots.iter().map(|root| root.uri.clone()).collect(),
    }
}

fn should_reset_client(err: &str) -> bool {
    err.contains("timed out")
        || err.contains("closed the stdio stream")
        || err.contains("Failed reading MCP server")
        || err.contains("Failed to write MCP stdio message")
        || err.contains("stdin pipe is unavailable")
        || err.contains("stdout pipe is unavailable")
}

fn server_health(server: &McpServerSession) -> String {
    if server.client.is_none() || server.initialize.is_none() {
        return "disconnected".to_string();
    }
    if server.last_error.is_some() {
        return "degraded".to_string();
    }
    "ready".to_string()
}

fn error_response(
    kind: &str,
    message: &str,
    metadata: Option<McpInvocationMetadata>,
) -> McpBridgeErrorResponse {
    McpBridgeErrorResponse {
        error: McpBridgeErrorBody {
            kind: kind.to_string(),
            message: message.to_string(),
            mcp: metadata,
        },
    }
}

fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|err| format!("Failed to generate MCP bridge token: {err}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
#[path = "tests/bridge.rs"]
mod tests;
