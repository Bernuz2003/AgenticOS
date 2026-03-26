use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agentic_kernel_macros::agentic_tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::backend::http::{HttpEndpoint, HttpRequestOptions};
use crate::config::kernel_config;
use crate::tool_registry::{
    HostExecutor, ToolBackendConfig, ToolBackendKind, ToolDescriptor, ToolRegistryEntry, ToolSource,
};

use super::api::{Tool, ToolResult};
use super::builtins::HostBuiltinRegistration;
use super::error::ToolError;
use super::invocation::{ToolContext, ToolInvocation};
use super::path_guard::{resolve_safe_path, resolve_safe_path_for_context, workspace_root};
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

fn run_with_timeout(
    cwd: &Path,
    program: &str,
    args: &[String],
    timeout_s: u64,
) -> Result<std::process::Output, String> {
    let mut wrapped = Command::new("timeout");
    wrapped
        .arg("--signal=KILL")
        .arg(format!("{}s", timeout_s.max(1)))
        .arg(program);
    for arg in args {
        wrapped.arg(arg);
    }
    wrapped.current_dir(cwd).output().map_err(|e| {
        format!(
            "SysCall Error: Failed to execute '{}' via timeout wrapper: {}",
            program, e
        )
    })
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

// ----------------------------------------------------
// T R A I T   I M P L E M E N T A T I O N S
// ----------------------------------------------------

pub struct BuiltinPythonTool;

impl Tool for BuiltinPythonTool {
    fn name(&self) -> &str {
        "python"
    }

    fn execute(
        &self,
        invocation: &ToolInvocation,
        context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let code = invocation
            .input
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or_default(); // Schema validation handles missing fields
        if code.trim().is_empty() {
            return Err(ToolError::InvalidInput(
                self.name().into(),
                "field 'code' cannot be empty".into(),
            ));
        }

        let clean_code = code
            .trim()
            .trim_start_matches("```python")
            .trim_start_matches("```")
            .trim_end_matches("```");

        let pid = context.pid.unwrap_or(0);
        let cfg = syscall_config();

        let root = workspace_root().map_err(|e| ToolError::Internal(e.to_string()))?;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_millis();
        let temp_filename = format!("agent_script_{}_{}.py", pid, ts);
        let script_path = root.join(temp_filename);

        fs::write(&script_path, clean_code).map_err(|e| {
            ToolError::ExecutionFailed(
                self.name().into(),
                format!("Failed to write temp file: {}", e),
            )
        })?;

        let run_result = match cfg.mode {
            SandboxMode::Host => run_host_python(&script_path, cfg.timeout_s)
                .map_err(|e| classify_timeout(self.name(), &e, cfg.timeout_s.max(1) * 1_000))?,
            SandboxMode::Container => match run_container_python(&script_path, cfg.timeout_s) {
                Ok(out) => Ok(out),
                Err(e) if cfg.allow_host_fallback => {
                    let host_out = run_host_python(&script_path, cfg.timeout_s).map_err(|he| {
                        classify_timeout(self.name(), &he, cfg.timeout_s.max(1) * 1_000)
                    })?;
                    Ok(format!(
                        "[Sandbox fallback: container->host due to error]\n{}\n{}",
                        e, host_out
                    ))
                }
                Err(e) => Err(e),
            }
            .map_err(|e| classify_timeout(self.name(), &e, cfg.timeout_s.max(1) * 1_000))?,
            SandboxMode::Wasm => {
                if cfg.allow_host_fallback {
                    let host_out = run_host_python(&script_path, cfg.timeout_s).map_err(|he| {
                        classify_timeout(self.name(), &he, cfg.timeout_s.max(1) * 1_000)
                    })?;
                    format!(
                        "[Sandbox fallback: wasm->host (wasm runner not configured)]\n{}",
                        host_out
                    )
                } else {
                    return Err(ToolError::ExecutionFailed(self.name().into(), "Sandbox mode 'wasm' selected but no wasm runner configured and host fallback disabled.".into()));
                }
            }
        };

        let _ = fs::remove_file(&script_path);

        Ok(ToolResult::json_with_text(
            json!({ "output": run_result.clone() }),
            run_result,
        ))
    }
}

fn builtin_python_tool_factory() -> Box<dyn Tool> {
    Box::new(BuiltinPythonTool)
}

pub(crate) fn python_host_builtin_registration() -> HostBuiltinRegistration {
    HostBuiltinRegistration::new(
        ToolRegistryEntry {
            descriptor: ToolDescriptor {
                name: "python".to_string(),
                aliases: vec![],
                description: "Execute Python code under the syscall sandbox policy.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["code"],
                    "properties": {
                        "code": {"type": "string"}
                    },
                    "additionalProperties": false
                }),
                input_example: Some(json!({"code": "print('hello')"})),
                output_schema: json!({
                    "type": "object",
                    "required": ["output"],
                    "properties": {
                        "output": {"type": "string"}
                    },
                    "additionalProperties": false
                }),
                allowed_callers: vec![
                    super::invocation::ToolCaller::AgentText,
                    super::invocation::ToolCaller::AgentSupervisor,
                    super::invocation::ToolCaller::Programmatic,
                ],
                backend_kind: ToolBackendKind::Host,
                capabilities: vec!["python".to_string(), "sandboxed".to_string()],
                dangerous: true,
                enabled: true,
                source: ToolSource::BuiltIn,
            },
            backend: ToolBackendConfig::Host {
                executor: HostExecutor::Python,
            },
        },
        builtin_python_tool_factory,
    )
}

pub struct BuiltinWriteFileTool;

impl Tool for BuiltinWriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn execute(
        &self,
        invocation: &ToolInvocation,
        context: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let filename = invocation
            .input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let content = invocation
            .input
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if filename.trim().is_empty() {
            return Err(ToolError::InvalidInput(
                self.name().into(),
                "field 'path' cannot be empty".into(),
            ));
        }

        let path = resolve_safe_path_for_context(filename, context)
            .map_err(|e| ToolError::ExecutionFailed(self.name().into(), e.to_string()))?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                ToolError::ExecutionFailed(
                    self.name().into(),
                    format!("Failed to create parent dir: {}", e),
                )
            })?;
        }

        fs::write(&path, content).map_err(|e| {
            ToolError::ExecutionFailed(self.name().into(), format!("Write failed: {}", e))
        })?;

        let message = format!(
            "Success: File '{}' written ({} bytes).",
            filename,
            content.len()
        );
        Ok(ToolResult::json_with_text(
            json!({
                "output": message.clone(),
                "path": filename,
                "bytes_written": content.len()
            }),
            message,
        ))
    }
}

fn builtin_write_file_tool_factory() -> Box<dyn Tool> {
    Box::new(BuiltinWriteFileTool)
}

pub(crate) fn write_file_host_builtin_registration() -> HostBuiltinRegistration {
    HostBuiltinRegistration::new(
        ToolRegistryEntry {
            descriptor: ToolDescriptor {
                name: "write_file".to_string(),
                aliases: vec![],
                description: "Write a file inside the workspace root.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["path", "content"],
                    "properties": {
                        "path": {"type": "string"},
                        "content": {"type": "string"}
                    },
                    "additionalProperties": false
                }),
                input_example: Some(json!({"path": "notes.txt", "content": "hello"})),
                output_schema: json!({
                    "type": "object",
                    "required": ["output", "path", "bytes_written"],
                    "properties": {
                        "output": {"type": "string"},
                        "path": {"type": "string"},
                        "bytes_written": {"type": "integer", "minimum": 0}
                    },
                    "additionalProperties": false
                }),
                allowed_callers: vec![
                    super::invocation::ToolCaller::AgentText,
                    super::invocation::ToolCaller::AgentSupervisor,
                    super::invocation::ToolCaller::Programmatic,
                ],
                backend_kind: ToolBackendKind::Host,
                capabilities: vec!["fs".to_string(), "write".to_string()],
                dangerous: true,
                enabled: true,
                source: ToolSource::BuiltIn,
            },
            backend: ToolBackendConfig::Host {
                executor: HostExecutor::WriteFile,
            },
        },
        builtin_write_file_tool_factory,
    )
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
    description = "Read a UTF-8 text file inside the workspace root.",
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
    description = "List files in the workspace root.",
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
    let workspace = workspace_root().map_err(|err| ToolError::Internal(err.to_string()))?;
    if ctx.permissions.path_scopes.is_empty() {
        return Err(ToolError::PolicyDenied(
            "list_files".into(),
            "no path scopes are available for this process".into(),
        ));
    }

    let mut roots = Vec::new();
    for scope in &ctx.permissions.path_scopes {
        let root = if scope == "." {
            workspace.clone()
        } else {
            resolve_safe_path(scope)
                .map_err(|err| ToolError::ExecutionFailed("list_files".into(), err))?
        };
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    Ok(roots)
}

fn to_workspace_relative_string(tool_name: &str, path: &Path) -> Result<String, ToolError> {
    let root = workspace_root().map_err(|err| ToolError::Internal(err.to_string()))?;
    let relative = path.strip_prefix(&root).map_err(|_| {
        ToolError::ExecutionFailed(
            tool_name.into(),
            format!("Path '{}' escaped the workspace root.", path.display()),
        )
    })?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
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
    description = "Evaluate a numeric expression through the Python sandbox.",
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
    let python_tool = BuiltinPythonTool;
    let invocation = ToolInvocation::new("python", json!({ "code": python_code }), None)
        .map_err(|err| ToolError::Internal(err.to_string()))?;
    let result = python_tool.execute(&invocation, ctx).map_err(|err| {
        ToolError::ExecutionFailed("calc".into(), format!("Calc evaluation failed: {err:?}"))
    })?;

    let output = result
        .output
        .get("output")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            ToolError::Internal(
                "calc expected the python tool to return a string output field".into(),
            )
        })?
        .to_string();

    Ok(CalcOutput { output })
}

pub struct RemoteHttpTool {
    pub name: String,
    pub backend: ToolBackendConfig,
}

impl Tool for RemoteHttpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn execute(
        &self,
        invocation: &ToolInvocation,
        _context: &ToolContext,
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

        enforce_remote_http_policy(self.name(), &endpoint)
            .map_err(|e| ToolError::PolicyDenied(self.name().into(), e))?;

        let path = if endpoint.base_path.is_empty() {
            "/"
        } else {
            endpoint.base_path.as_str()
        };

        let response = endpoint
            .request_json_with_options(
                &method.to_ascii_uppercase(),
                path,
                Some(&invocation.input),
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
            return Err(ToolError::ExecutionFailed(
                self.name().into(),
                format!(
                    "HTTP {} ({}). {}",
                    response.status_code, response.status_line, response.body
                ),
            ));
        }

        if let Some(json_body) = response.json {
            let display_text = json_body
                .get("output")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
                .or_else(|| serde_json::to_string_pretty(&json_body).ok())
                .unwrap_or_else(|| "Valid JSON response".into());
            Ok(ToolResult::json_with_text(json_body, display_text))
        } else {
            Ok(ToolResult::plain_text(response.body))
        }
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
