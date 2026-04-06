use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::config::{McpServerConfig, McpTransportConfig};
use crate::mcp::jsonrpc;
use crate::mcp::models::{
    McpCallToolResult, McpImplementation, McpInitializeResult, McpPromptDefinition,
    McpResourceDefinition, McpRoot, McpServerCapabilities, McpToolAnnotations, McpToolDefinition,
    MCP_PROTOCOL_VERSION,
};

#[derive(Debug)]
enum InboundMessage {
    Json(Value),
    ReadError(String),
    Closed,
}

#[derive(Debug)]
pub(crate) struct McpStdioClient {
    server_id: String,
    child: Child,
    stdin: ChildStdin,
    inbound_rx: mpsc::Receiver<InboundMessage>,
    timeout: Duration,
    next_id: u64,
    current_roots: Vec<McpRoot>,
}

impl McpStdioClient {
    pub(crate) fn connect(config: &McpServerConfig) -> Result<Self, String> {
        let McpTransportConfig::Stdio {
            command,
            args,
            cwd,
            env,
            timeout_ms,
        } = &config.transport;

        if command.trim().is_empty() {
            return Err(format!(
                "MCP server '{}' stdio command is empty.",
                config.id
            ));
        }

        let mut child = Command::new(command);
        child.args(args);
        child.stdin(Stdio::piped());
        child.stdout(Stdio::piped());
        child.stderr(Stdio::piped());
        if let Some(cwd) = cwd {
            child.current_dir(cwd);
        }
        for (key, value) in env {
            child.env(key, value);
        }

        let mut child = child.spawn().map_err(|err| {
            format!(
                "Failed to spawn MCP server '{}' using command '{}': {}",
                config.id, command, err
            )
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("MCP server '{}' stdin pipe is unavailable.", config.id))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("MCP server '{}' stdout pipe is unavailable.", config.id))?;

        let (inbound_tx, inbound_rx) = mpsc::channel();
        let server_id = config.id.clone();
        thread::Builder::new()
            .name(format!("mcp-stdout-{}", server_id))
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => {
                            let _ = inbound_tx.send(InboundMessage::Closed);
                            break;
                        }
                        Ok(_) => {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            match serde_json::from_str::<Value>(trimmed) {
                                Ok(message) => {
                                    let _ = inbound_tx.send(InboundMessage::Json(message));
                                }
                                Err(err) => {
                                    let _ = inbound_tx.send(InboundMessage::ReadError(format!(
                                        "Invalid MCP JSON from server '{}': {}",
                                        server_id, err
                                    )));
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            let _ = inbound_tx.send(InboundMessage::ReadError(format!(
                                "Failed reading MCP server '{}': {}",
                                server_id, err
                            )));
                            break;
                        }
                    }
                }
            })
            .map_err(|err| format!("Failed to start MCP stdout reader thread: {err}"))?;

        if let Some(stderr) = child.stderr.take() {
            let server_id = config.id.clone();
            let _ = thread::Builder::new()
                .name(format!("mcp-stderr-{}", server_id))
                .spawn(move || {
                    let mut reader = BufReader::new(stderr);
                    loop {
                        let mut line = String::new();
                        match reader.read_line(&mut line) {
                            Ok(0) => break,
                            Ok(_) => {
                                let message = line.trim();
                                if !message.is_empty() {
                                    tracing::debug!(server_id, message, "MCP_STDERR");
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });
        }

        Ok(Self {
            server_id: config.id.clone(),
            child,
            stdin,
            inbound_rx,
            timeout: Duration::from_millis((*timeout_ms).max(1)),
            next_id: 1,
            current_roots: Vec::new(),
        })
    }

    pub(crate) fn initialize(&mut self) -> Result<McpInitializeResult, String> {
        let result = self.request(
            "initialize",
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {
                    "roots": {
                        "listChanged": true
                    }
                },
                "clientInfo": {
                    "name": "AgenticOS",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )?;

        let initialize = parse_initialize_result(&result)?;
        if initialize.protocol_version != MCP_PROTOCOL_VERSION {
            return Err(format!(
                "MCP server requested unsupported protocol version '{}'.",
                initialize.protocol_version
            ));
        }

        self.write_message(&jsonrpc::notification("notifications/initialized", None))?;
        Ok(initialize)
    }

    pub(crate) fn list_tools(&mut self) -> Result<Vec<McpToolDefinition>, String> {
        let result = self.request("tools/list", json!({}))?;
        parse_tools(&result)
    }

    pub(crate) fn list_resources(
        &mut self,
        capabilities: &McpServerCapabilities,
    ) -> Result<Vec<McpResourceDefinition>, String> {
        if !capabilities.resources_list_changed && !capabilities.resources_subscribe {
            let result = self.request("resources/list", json!({}))?;
            return parse_resources(&result);
        }
        let result = self.request("resources/list", json!({}))?;
        parse_resources(&result)
    }

    pub(crate) fn list_prompts(
        &mut self,
        _capabilities: &McpServerCapabilities,
    ) -> Result<Vec<McpPromptDefinition>, String> {
        let result = self.request("prompts/list", json!({}))?;
        parse_prompts(&result)
    }

    pub(crate) fn call_tool(
        &mut self,
        tool_name: &str,
        input: Value,
        roots: Vec<McpRoot>,
    ) -> Result<McpCallToolResult, String> {
        if self.current_roots != roots {
            self.current_roots = roots;
            self.write_message(&jsonrpc::notification(
                "notifications/roots/list_changed",
                None,
            ))?;
        }

        let result = self.request(
            "tools/call",
            json!({
                "name": tool_name,
                "arguments": input,
            }),
        )?;

        parse_call_tool_result(&result)
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let request_id = self.next_request_id();
        self.write_message(&jsonrpc::request(request_id, method, params))?;

        let deadline = Instant::now() + self.timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.abort_request(method);
                return Err(format!(
                    "MCP request '{}' timed out after {} ms; stdio server was cancelled.",
                    method,
                    self.timeout.as_millis()
                ));
            }

            let message = match self.inbound_rx.recv_timeout(remaining) {
                Ok(message) => message,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.abort_request(method);
                    return Err(format!(
                        "MCP request '{}' timed out waiting for response after {} ms; stdio server was cancelled.",
                        method,
                        self.timeout.as_millis()
                    ));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(format!(
                        "MCP server '{}' disconnected while handling '{}'.",
                        self.server_id, method
                    ));
                }
            };

            match message {
                InboundMessage::Json(message) => {
                    if let Some(id) = jsonrpc::extract_response_id(&message) {
                        if id != request_id {
                            continue;
                        }
                        if let Some(error) = message.get("error") {
                            let code = error.get("code").and_then(Value::as_i64).unwrap_or(-32_603);
                            let detail = error
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown MCP error");
                            return Err(format!("MCP error {}: {}", code, detail));
                        }
                        return message
                            .get("result")
                            .cloned()
                            .ok_or_else(|| "MCP response is missing result.".to_string());
                    }

                    if let Some(server_method) = jsonrpc::extract_request_method(&message) {
                        self.handle_server_request(server_method, &message)?;
                        continue;
                    }

                    if jsonrpc::extract_notification_method(&message).is_some() {
                        continue;
                    }
                }
                InboundMessage::ReadError(err) => return Err(err),
                InboundMessage::Closed => {
                    return Err(format!(
                        "MCP server closed the stdio stream while handling '{}'.",
                        method
                    ));
                }
            }
        }
    }

    fn handle_server_request(&mut self, method: &str, message: &Value) -> Result<(), String> {
        let Some(id) = message.get("id").and_then(Value::as_u64) else {
            return Ok(());
        };
        let response = match method {
            "roots/list" => jsonrpc::success_response(
                id,
                json!({
                    "roots": self.current_roots,
                }),
            ),
            "ping" => jsonrpc::success_response(id, json!({})),
            _ => {
                jsonrpc::error_response(id, -32_601, "Method not supported by AgenticOS MCP client")
            }
        };
        self.write_message(&response)
    }

    fn write_message(&mut self, value: &Value) -> Result<(), String> {
        let serialized = serde_json::to_string(value)
            .map_err(|err| format!("Failed to serialize MCP message: {err}"))?;
        self.stdin
            .write_all(serialized.as_bytes())
            .and_then(|_| self.stdin.write_all(b"\n"))
            .and_then(|_| self.stdin.flush())
            .map_err(|err| format!("Failed to write MCP stdio message: {err}"))
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn abort_request(&mut self, method: &str) {
        if let Err(err) = self.child.kill() {
            tracing::debug!(
                server_id = self.server_id,
                request_method = method,
                %err,
                "MCP timeout cancellation kill failed"
            );
        }
        let _ = self.child.wait();
    }
}

impl Drop for McpStdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn parse_initialize_result(result: &Value) -> Result<McpInitializeResult, String> {
    Ok(McpInitializeResult {
        protocol_version: result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .ok_or_else(|| "MCP initialize result is missing protocolVersion.".to_string())?
            .to_string(),
        capabilities: parse_server_capabilities(
            result
                .get("capabilities")
                .ok_or_else(|| "MCP initialize result is missing capabilities.".to_string())?,
        ),
        server_info: parse_implementation(
            result
                .get("serverInfo")
                .ok_or_else(|| "MCP initialize result is missing serverInfo.".to_string())?,
        )?,
        instructions: result
            .get("instructions")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    })
}

fn parse_server_capabilities(value: &Value) -> McpServerCapabilities {
    McpServerCapabilities {
        tools_list_changed: value
            .get("tools")
            .and_then(|tools| tools.get("listChanged"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        resources_list_changed: value
            .get("resources")
            .and_then(|resources| resources.get("listChanged"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        resources_subscribe: value
            .get("resources")
            .and_then(|resources| resources.get("subscribe"))
            .is_some(),
        prompts_list_changed: value
            .get("prompts")
            .and_then(|prompts| prompts.get("listChanged"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn parse_implementation(value: &Value) -> Result<McpImplementation, String> {
    Ok(McpImplementation {
        name: value
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "MCP implementation is missing name.".to_string())?
            .to_string(),
        version: value
            .get("version")
            .and_then(Value::as_str)
            .ok_or_else(|| "MCP implementation is missing version.".to_string())?
            .to_string(),
    })
}

fn parse_tools(result: &Value) -> Result<Vec<McpToolDefinition>, String> {
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| "MCP tools/list result is missing tools.".to_string())?;
    let mut values = Vec::new();
    for tool in tools {
        values.push(McpToolDefinition {
            name: tool
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "MCP tool is missing name.".to_string())?
                .to_string(),
            title: tool
                .get("title")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            description: tool
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            input_schema: tool
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object"})),
            output_schema: tool.get("outputSchema").cloned(),
            annotations: parse_tool_annotations(tool.get("annotations")),
        });
    }
    Ok(values)
}

fn parse_tool_annotations(value: Option<&Value>) -> McpToolAnnotations {
    let Some(value) = value else {
        return McpToolAnnotations::default();
    };
    McpToolAnnotations {
        title: value
            .get("title")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        read_only_hint: value
            .get("readOnlyHint")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        destructive_hint: value
            .get("destructiveHint")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        idempotent_hint: value
            .get("idempotentHint")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        open_world_hint: value
            .get("openWorldHint")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn parse_resources(result: &Value) -> Result<Vec<McpResourceDefinition>, String> {
    let resources = result
        .get("resources")
        .and_then(Value::as_array)
        .ok_or_else(|| "MCP resources/list result is missing resources.".to_string())?;
    Ok(resources
        .iter()
        .filter_map(|resource| {
            Some(McpResourceDefinition {
                name: resource.get("name")?.as_str()?.to_string(),
                title: resource
                    .get("title")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                uri: resource.get("uri")?.as_str()?.to_string(),
                description: resource
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                mime_type: resource
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            })
        })
        .collect())
}

fn parse_prompts(result: &Value) -> Result<Vec<McpPromptDefinition>, String> {
    let prompts = result
        .get("prompts")
        .and_then(Value::as_array)
        .ok_or_else(|| "MCP prompts/list result is missing prompts.".to_string())?;
    Ok(prompts
        .iter()
        .filter_map(|prompt| {
            Some(McpPromptDefinition {
                name: prompt.get("name")?.as_str()?.to_string(),
                title: prompt
                    .get("title")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                description: prompt
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            })
        })
        .collect())
}

fn parse_call_tool_result(result: &Value) -> Result<McpCallToolResult, String> {
    Ok(McpCallToolResult {
        content: result
            .get("content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        structured_content: result.get("structuredContent").cloned(),
        is_error: result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}
