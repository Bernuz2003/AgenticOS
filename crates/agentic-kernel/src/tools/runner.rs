use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agentic_kernel_macros::agentic_tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::backend::{HttpEndpoint, HttpRequestOptions};
use crate::config::kernel_config;
use crate::mcp::models::{McpBridgeErrorResponse, McpBridgeInvocationRequest};
use crate::tool_registry::{ToolBackendConfig, ToolInteropDescriptor};

use super::api::{Tool, ToolResult};
use super::error::ToolError;
use super::host_exec::run_with_timeout;
use super::invocation::{ToolContext, ToolInvocation};
use super::path_guard::{
    display_path, resolve_context_grant_roots, resolve_safe_path_for_context,
    resolve_safe_write_path_for_context, workspace_root,
};
use super::policy::{
    enforce_remote_http_policy, remote_http_max_request_bytes, remote_http_max_response_bytes,
    syscall_config, SandboxMode,
};

fn truncate_output(text: &str) -> String {
    let limit = kernel_config().tools.output_truncate_len;
    if text.len() > limit {
        let mut end = limit;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}... (Output Truncated)", &text[..end])
    } else if text.trim().is_empty() {
        "Done (No Output)".to_string()
    } else {
        text.to_string()
    }
}

fn run_host_python(script_path: &Path, timeout_s: u64) -> Result<String, String> {
    let cwd = workspace_root().map_err(|e| format!("Safe path error: {}", e))?;
    let script_name = script_path
        .file_name()
        .ok_or_else(|| "SysCall Error: Invalid script filename.".to_string())?
        .to_string_lossy()
        .to_string();

    let output = run_with_timeout(&cwd, "python3", &[script_name], timeout_s)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.code() == Some(124) || output.status.code() == Some(137) {
        return Err(format!(
            "SysCall Error: Python execution timed out after {}s.",
            timeout_s.max(1)
        ));
    }

    if output.status.success() {
        if stderr.trim().is_empty() {
            Ok(truncate_output(&stdout))
        } else {
            Ok(truncate_output(&format!(
                "Output:\n{}\nErrors:\n{}",
                stdout, stderr
            )))
        }
    } else {
        Err(truncate_output(&format!(
            "SysCall Error: Python failed (status={:?}).\n{}{}",
            output.status.code(),
            if stdout.is_empty() {
                ""
            } else {
                &format!("stdout:\n{}\n", stdout)
            },
            if stderr.is_empty() {
                ""
            } else {
                &format!("stderr:\n{}", stderr)
            }
        )))
    }
}

fn run_container_python(script_path: &Path, timeout_s: u64) -> Result<String, String> {
    let cwd = workspace_root().map_err(|e| format!("Safe path error: {}", e))?;
    let script_name = script_path
        .file_name()
        .ok_or_else(|| "SysCall Error: Invalid script filename.".to_string())?
        .to_string_lossy()
        .to_string();

    let volume = format!("{}:/workspace", cwd.display());
    let args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--network".to_string(),
        "none".to_string(),
        "-m".to_string(),
        "256m".to_string(),
        "--cpus".to_string(),
        "1".to_string(),
        "-v".to_string(),
        volume,
        "-w".to_string(),
        "/workspace".to_string(),
        "python:3.11-alpine".to_string(),
        "python3".to_string(),
        script_name,
    ];

    let output = run_with_timeout(&cwd, "docker", &args, timeout_s)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.code() == Some(124) || output.status.code() == Some(137) {
        return Err(format!(
            "SysCall Error: Container execution timed out after {}s.",
            timeout_s.max(1)
        ));
    }

    if output.status.success() {
        Ok(truncate_output(&stdout))
    } else {
        Err(truncate_output(&format!(
            "SysCall Error: Container runner failed (status={:?}).\n{}{}",
            output.status.code(),
            if stdout.is_empty() {
                ""
            } else {
                &format!("stdout:\n{}\n", stdout)
            },
            if stderr.is_empty() {
                ""
            } else {
                &format!("stderr:\n{}", stderr)
            }
        )))
    }
}

fn classify_timeout(tool_name: &str, detail: &str, timeout_ms: u64) -> ToolError {
    if detail.contains("timed out after") {
        ToolError::Timeout(tool_name.to_string(), timeout_ms)
    } else {
        ToolError::ExecutionFailed(tool_name.to_string(), detail.to_string())
    }
}

fn classify_remote_http_failure(tool_name: &str, detail: String, timeout_ms: u64) -> ToolError {
    if detail.contains("timed out after") {
        ToolError::Timeout(tool_name.to_string(), timeout_ms)
    } else if detail.contains("Failed to resolve")
        || detail.contains("No address resolved")
        || detail.contains("Failed to connect")
    {
        ToolError::BackendUnavailable(tool_name.to_string(), detail)
    } else {
        ToolError::ExecutionFailed(tool_name.to_string(), detail)
    }
}

fn execute_python_code(
    tool_name: &str,
    code: &str,
    context: &ToolContext,
) -> Result<String, ToolError> {
    let clean_code = code
        .trim()
        .trim_start_matches("```python")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if clean_code.is_empty() {
        return Err(ToolError::InvalidInput(
            tool_name.into(),
            "field 'code' cannot be empty".into(),
        ));
    }

    let pid = context.pid.unwrap_or(0);
    let cfg = syscall_config();

    let root = workspace_root().map_err(|err| ToolError::Internal(err.to_string()))?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis();
    let temp_filename = format!("agent_script_{}_{}.py", pid, ts);
    let script_path = root.join(temp_filename);

    fs::write(&script_path, clean_code).map_err(|err| {
        ToolError::ExecutionFailed(
            tool_name.into(),
            format!("Failed to write temp file: {err}"),
        )
    })?;

    let run_result = match cfg.mode {
        SandboxMode::Host => run_host_python(&script_path, cfg.timeout_s)
            .map_err(|err| classify_timeout(tool_name, &err, cfg.timeout_s.max(1) * 1_000))?,
        SandboxMode::Container => match run_container_python(&script_path, cfg.timeout_s) {
            Ok(out) => Ok(out),
            Err(err) if cfg.allow_host_fallback => {
                let host_out =
                    run_host_python(&script_path, cfg.timeout_s).map_err(|host_err| {
                        classify_timeout(tool_name, &host_err, cfg.timeout_s.max(1) * 1_000)
                    })?;
                Ok(format!(
                    "[Sandbox fallback: container->host due to error]\n{}\n{}",
                    err, host_out
                ))
            }
            Err(err) => Err(err),
        }
        .map_err(|err| classify_timeout(tool_name, &err, cfg.timeout_s.max(1) * 1_000))?,
        SandboxMode::Wasm => {
            if cfg.allow_host_fallback {
                let host_out =
                    run_host_python(&script_path, cfg.timeout_s).map_err(|host_err| {
                        classify_timeout(tool_name, &host_err, cfg.timeout_s.max(1) * 1_000)
                    })?;
                format!(
                    "[Sandbox fallback: wasm->host (wasm runner not configured)]\n{}",
                    host_out
                )
            } else {
                let _ = fs::remove_file(&script_path);
                return Err(ToolError::ExecutionFailed(
                    tool_name.into(),
                    "Sandbox mode 'wasm' selected but no wasm runner configured and host fallback disabled.".into(),
                ));
            }
        }
    };

    let _ = fs::remove_file(&script_path);
    Ok(run_result)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PythonInput {
    code: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct PythonOutput {
    output: String,
}

#[agentic_tool(
    name = "python",
    description = "Execute Python code under the syscall sandbox policy and return stdout/stderr as text.",
    input_example = serde_json::json!({"code": "print('hello')"}),
    capabilities = ["python", "sandboxed"],
    dangerous = true,
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn python(input: PythonInput, ctx: &ToolContext) -> Result<PythonOutput, ToolError> {
    let output = execute_python_code("python", &input.code, ctx)?;
    Ok(PythonOutput { output })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WriteFileInput {
    path: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct WriteFileOutput {
    output: String,
    path: String,
    bytes_written: usize,
}

#[agentic_tool(
    name = "write_file",
    description = "Write UTF-8 text to a file inside the process-scoped workspace, creating parent directories when needed.",
    input_example = serde_json::json!({"path": "notes/todo.txt", "content": "hello"}),
    capabilities = ["fs", "write"],
    dangerous = true,
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn write_file(input: WriteFileInput, ctx: &ToolContext) -> Result<WriteFileOutput, ToolError> {
    if input.path.trim().is_empty() {
        return Err(ToolError::InvalidInput(
            "write_file".into(),
            "field 'path' cannot be empty".into(),
        ));
    }

    let path = resolve_safe_write_path_for_context(&input.path, ctx)
        .map_err(|err| ToolError::ExecutionFailed("write_file".into(), err.to_string()))?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ToolError::ExecutionFailed(
                "write_file".into(),
                format!("Failed to create parent dir: {err}"),
            )
        })?;
    }

    fs::write(&path, &input.content).map_err(|err| {
        ToolError::ExecutionFailed("write_file".into(), format!("Write failed: {err}"))
    })?;

    let output = format!(
        "Success: File '{}' written ({} bytes).",
        input.path,
        input.content.len()
    );

    Ok(WriteFileOutput {
        output,
        path: input.path,
        bytes_written: input.content.len(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadFileInput {
    path: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct ReadFileOutput {
    output: String,
    path: String,
}

#[agentic_tool(
    name = "read_file",
    description = "Read a UTF-8 text file inside the process-scoped workspace.",
    input_example = serde_json::json!({"path": "notes/todo.txt"}),
    capabilities = ["fs", "read"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn read_file(input: ReadFileInput, ctx: &ToolContext) -> Result<ReadFileOutput, ToolError> {
    if input.path.trim().is_empty() {
        return Err(ToolError::InvalidInput(
            "read_file".into(),
            "field 'path' cannot be empty".into(),
        ));
    }

    let path = resolve_safe_path_for_context(&input.path, ctx)
        .map_err(|err| ToolError::ExecutionFailed("read_file".into(), err.to_string()))?;

    let meta = fs::metadata(&path).map_err(|err| {
        ToolError::ExecutionFailed("read_file".into(), format!("Read failed: {err}"))
    })?;
    if meta.len() > 1024 * 1024 {
        return Err(ToolError::ExecutionFailed(
            "read_file".into(),
            "Refusing to read files larger than 1MB.".into(),
        ));
    }

    let content = fs::read_to_string(&path).map_err(|err| {
        ToolError::ExecutionFailed("read_file".into(), format!("Read failed: {err}"))
    })?;

    Ok(ReadFileOutput {
        output: content,
        path: input.path,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
struct ListFilesInput {}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct ListFilesOutput {
    output: String,
    entries: Vec<String>,
}

#[agentic_tool(
    name = "list_files",
    description = "List the direct children of the process-scoped workspace roots.",
    input_example = serde_json::json!({}),
    capabilities = ["fs", "list"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn list_files(_input: ListFilesInput, ctx: &ToolContext) -> Result<ListFilesOutput, ToolError> {
    let roots = resolve_list_roots(ctx)?;
    let mut files = Vec::new();
    for root in roots {
        if root.is_file() {
            files.push(to_workspace_relative_string("list_files", &root)?);
            continue;
        }
        let entries = fs::read_dir(&root).map_err(|err| {
            ToolError::ExecutionFailed("list_files".into(), format!("LS failed: {err}"))
        })?;

        for entry in entries.flatten() {
            files.push(to_workspace_relative_string("list_files", &entry.path())?);
        }
    }
    files.sort();
    files.dedup();

    let output = if files.is_empty() {
        "Workspace is empty.".to_string()
    } else {
        format!("Files:\n- {}", files.join("\n- "))
    };

    Ok(ListFilesOutput {
        output,
        entries: files,
    })
}

fn resolve_list_roots(ctx: &ToolContext) -> Result<Vec<std::path::PathBuf>, ToolError> {
    resolve_context_grant_roots(ctx)
        .map_err(|err| ToolError::ExecutionFailed("list_files".into(), err))
}

fn to_workspace_relative_string(tool_name: &str, path: &Path) -> Result<String, ToolError> {
    display_path(path).map_err(|err| ToolError::ExecutionFailed(tool_name.into(), err))
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CalcInput {
    expression: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct CalcOutput {
    output: String,
}

#[agentic_tool(
    name = "calc",
    description = "Evaluate a numeric expression through the Python sandbox and return the resulting text.",
    input_example = serde_json::json!({"expression": "1 + 2 * 3"}),
    capabilities = ["math", "python"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn calc(input: CalcInput, ctx: &ToolContext) -> Result<CalcOutput, ToolError> {
    if input.expression.trim().is_empty() {
        return Err(ToolError::InvalidInput(
            "calc".into(),
            "field 'expression' cannot be empty".into(),
        ));
    }

    let python_code = format!("print({})", input.expression);
    let output = execute_python_code("calc", &python_code, ctx)?;

    Ok(CalcOutput { output })
}

pub struct RemoteHttpTool {
    pub name: String,
    pub backend: ToolBackendConfig,
    pub interop: Option<ToolInteropDescriptor>,
}

impl Tool for RemoteHttpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn execute(
        &self,
        invocation: &ToolInvocation,
        context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let ToolBackendConfig::RemoteHttp {
            url,
            method,
            timeout_ms,
            headers,
        } = &self.backend
        else {
            return Err(ToolError::Internal(format!(
                "Wrong backend for RemoteHttpTool: {:?}",
                self.backend
            )));
        };

        let endpoint = HttpEndpoint::parse(url).map_err(|e| {
            ToolError::ExecutionFailed(self.name().into(), format!("Invalid endpoint: {}", e))
        })?;

        if !is_internal_mcp_bridge_endpoint(self.interop.as_ref(), &endpoint) {
            enforce_remote_http_policy(self.name(), &endpoint)
                .map_err(|e| ToolError::PolicyDenied(self.name().into(), e))?;
        }

        let path = if endpoint.base_path.is_empty() {
            "/"
        } else {
            endpoint.base_path.as_str()
        };

        let response = endpoint
            .request_json_with_options(
                &method.to_ascii_uppercase(),
                path,
                Some(&remote_http_payload(
                    invocation,
                    context,
                    self.interop.as_ref(),
                )),
                HttpRequestOptions {
                    timeout_ms: *timeout_ms,
                    max_request_bytes: remote_http_max_request_bytes(),
                    max_response_bytes: remote_http_max_response_bytes(),
                    extra_headers: Some(headers),
                },
            )
            .map_err(|e| {
                classify_remote_http_failure(
                    self.name(),
                    format!("Request failed: {}", e),
                    *timeout_ms,
                )
            })?;

        if !(200..300).contains(&response.status_code) {
            return Err(classify_http_response_failure(
                self.name(),
                response.status_code,
                response.status_line,
                response.body,
                response.json,
                *timeout_ms,
                self.interop.as_ref(),
            ));
        }

        if let Some(json_body) = response.json {
            let display_text = json_body
                .get("output")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
                .or_else(|| serde_json::to_string_pretty(&json_body).ok())
                .unwrap_or_else(|| "Valid JSON response".into());
            let warnings = json_body
                .get("warnings")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Ok(ToolResult {
                output: json_body,
                display_text: Some(display_text),
                warnings,
            })
        } else {
            Ok(ToolResult::plain_text(response.body))
        }
    }
}

fn remote_http_payload(
    invocation: &ToolInvocation,
    context: &ToolContext,
    interop: Option<&ToolInteropDescriptor>,
) -> Value {
    if interop.is_some_and(|interop| interop.provider == "mcp") {
        return serde_json::to_value(McpBridgeInvocationRequest {
            input: invocation.input.clone(),
            context: context.clone(),
        })
        .unwrap_or_else(|_| invocation.input.clone());
    }

    invocation.input.clone()
}

fn classify_http_response_failure(
    tool_name: &str,
    status_code: u16,
    status_line: String,
    body: String,
    json_body: Option<Value>,
    timeout_ms: u64,
    interop: Option<&ToolInteropDescriptor>,
) -> ToolError {
    if interop.is_some_and(|interop| interop.provider == "mcp") {
        if let Some(json_body) = json_body {
            if let Ok(parsed) = serde_json::from_value::<McpBridgeErrorResponse>(json_body) {
                return match parsed.error.kind.as_str() {
                    "policy_denied" | "roots_denied" => {
                        ToolError::PolicyDenied(tool_name.to_string(), parsed.error.message)
                    }
                    "mcp_result_validation_failed" => ToolError::OutputSchemaViolation(
                        tool_name.to_string(),
                        parsed.error.message,
                    ),
                    "mcp_transport_failed" if status_code == 504 => {
                        ToolError::Timeout(tool_name.to_string(), timeout_ms)
                    }
                    "mcp_transport_failed" => {
                        ToolError::BackendUnavailable(tool_name.to_string(), parsed.error.message)
                    }
                    _ => ToolError::ExecutionFailed(tool_name.to_string(), parsed.error.message),
                };
            }
        }
    }

    ToolError::ExecutionFailed(
        tool_name.to_string(),
        format!("HTTP {} ({}). {}", status_code, status_line, body),
    )
}

fn is_internal_mcp_bridge_endpoint(
    interop: Option<&ToolInteropDescriptor>,
    endpoint: &HttpEndpoint,
) -> bool {
    interop.is_some_and(|interop| {
        interop.provider == "mcp" && matches!(endpoint.host.as_str(), "127.0.0.1" | "localhost")
    })
}

#[cfg(test)]
#[path = "tests/runner.rs"]
mod tests;
