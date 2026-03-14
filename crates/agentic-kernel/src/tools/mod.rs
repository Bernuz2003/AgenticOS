use std::fs;
use std::net::{IpAddr, ToSocketAddrs};
use std::time::Instant;

use crate::backend::http::{HttpEndpoint, HttpRequestOptions};
use crate::config::kernel_config;
use crate::tool_registry::{ToolBackendConfig, ToolRegistry};
use serde_json::{json, Value};

mod audit;
mod path_guard;
mod policy;
mod runner;

use audit::append_audit_log;
use path_guard::workspace_root;
use policy::{rate_limit_postcheck, rate_limit_precheck, syscall_config};
use runner::{execute_python_with_policy, handle_list_files, handle_read_file, handle_write_file};

pub(crate) use policy::SyscallRateMap;

/// Remove stale `agent_script_*.py` temp files left by previous crashes.
/// Called once at kernel boot.
pub fn cleanup_stale_temp_scripts() {
    let root = match workspace_root() {
        Ok(p) => p,
        Err(_) => return,
    };
    let prefix = &kernel_config().tools.temp_script_prefix;
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(prefix) && name.ends_with(".py") {
            if let Err(e) = fs::remove_file(entry.path()) {
                tracing::warn!(file = %name, %e, "failed to remove stale temp script");
            } else {
                tracing::debug!(file = %name, "removed stale temp script");
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct SysCallOutcome {
    pub output: String,
    pub success: bool,
    pub duration_ms: u128,
    pub should_kill_process: bool,
}

struct ToolInvocation {
    name: String,
    input: Value,
}

pub fn handle_syscall(
    command_block: &str,
    pid: u64,
    rate_map: &mut SyscallRateMap,
    registry: &ToolRegistry,
) -> SysCallOutcome {
    let cfg = syscall_config();
    let start = Instant::now();
    let clean_cmd = command_block.trim();

    if let Err(e) = rate_limit_precheck(pid, cfg, rate_map) {
        append_audit_log(
            pid,
            cfg.mode,
            clean_cmd,
            false,
            start.elapsed().as_millis(),
            true,
            &e,
        );
        return SysCallOutcome {
            output: e,
            success: false,
            duration_ms: start.elapsed().as_millis(),
            should_kill_process: true,
        };
    }

    let exec_result: Result<String, String> = parse_tool_invocation(clean_cmd)
        .and_then(|invocation| execute_registered_tool(&invocation, pid, cfg, registry));

    let (success, output) = match exec_result {
        Ok(msg) => (true, msg),
        Err(err) => (false, err),
    };

    let kill_from_burst = rate_limit_postcheck(pid, success, cfg, rate_map);
    let mut final_output = output;
    if kill_from_burst {
        final_output.push_str("\nSysCall Guard: process killed due to repeated syscall failures.");
    }

    append_audit_log(
        pid,
        cfg.mode,
        clean_cmd,
        success,
        start.elapsed().as_millis(),
        kill_from_burst,
        &final_output,
    );

    SysCallOutcome {
        output: final_output,
        success,
        duration_ms: start.elapsed().as_millis(),
        should_kill_process: kill_from_burst,
    }
}

pub(crate) fn validates_tool_invocation(command_block: &str) -> bool {
    parse_tool_invocation(command_block).is_ok()
}

fn parse_tool_invocation(command_block: &str) -> Result<ToolInvocation, String> {
    if let Some(rest) = command_block.strip_prefix("TOOL:") {
        return parse_canonical_tool_invocation(rest);
    }

    if let Some(code) = command_block.strip_prefix("PYTHON:") {
        return Ok(ToolInvocation {
            name: "python".to_string(),
            input: json!({"code": code.trim()}),
        });
    }

    if let Some(args) = command_block.strip_prefix("WRITE_FILE:") {
        let parts: Vec<&str> = args.splitn(2, '|').collect();
        if parts.len() < 2 {
            return Err("SysCall Error: Usage [[WRITE_FILE: filename | content]]".to_string());
        }
        return Ok(ToolInvocation {
            name: "write_file".to_string(),
            input: json!({
                "path": parts[0].trim(),
                "content": parts[1].trim_start(),
            }),
        });
    }

    if let Some(path) = command_block.strip_prefix("READ_FILE:") {
        return Ok(ToolInvocation {
            name: "read_file".to_string(),
            input: json!({"path": path.trim()}),
        });
    }

    if command_block == "LS" || command_block.starts_with("LS ") {
        return Ok(ToolInvocation {
            name: "list_files".to_string(),
            input: json!({}),
        });
    }

    if let Some(expr) = command_block.strip_prefix("CALC:") {
        return Ok(ToolInvocation {
            name: "calc".to_string(),
            input: json!({"expression": expr.trim()}),
        });
    }

    Err("SysCall Error: Unknown Tool or forbidden command.".to_string())
}

fn parse_canonical_tool_invocation(rest: &str) -> Result<ToolInvocation, String> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Err("SysCall Error: TOOL invocation requires a tool name.".to_string());
    }

    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or_default().trim();
    let payload_text = parts.next().unwrap_or("{}").trim();
    let payload = if payload_text.is_empty() {
        json!({})
    } else {
        serde_json::from_str::<Value>(payload_text)
            .map_err(|e| format!("SysCall Error: TOOL payload must be valid JSON: {}", e))?
    };

    if !payload.is_object() {
        return Err("SysCall Error: TOOL payload must be a JSON object.".to_string());
    }

    Ok(ToolInvocation {
        name: name.to_string(),
        input: payload,
    })
}

fn execute_registered_tool(
    invocation: &ToolInvocation,
    pid: u64,
    cfg: policy::SysCallConfig,
    registry: &ToolRegistry,
) -> Result<String, String> {
    let entry = registry
        .resolve_invocation_name(&invocation.name)
        .ok_or_else(|| {
            format!(
                "SysCall Error: Tool '{}' is not registered.",
                invocation.name
            )
        })?;
    let descriptor = &entry.descriptor;

    if !descriptor.enabled {
        return Err(format!(
            "SysCall Error: Tool '{}' is disabled.",
            descriptor.name
        ));
    }

    match descriptor.name.as_str() {
        "python" => {
            let code = required_str(&invocation.input, "code")?;
            execute_python_with_policy(code, pid, cfg)
        }
        "write_file" => {
            let path = required_str(&invocation.input, "path")?;
            let content = required_str(&invocation.input, "content")?;
            handle_write_file_json(path, content)
        }
        "read_file" => {
            let path = required_str(&invocation.input, "path")?;
            handle_read_file(path)
        }
        "list_files" => handle_list_files(),
        "calc" => {
            let expression = required_str(&invocation.input, "expression")?;
            execute_python_with_policy(&format!("print({})", expression), pid, cfg)
        }
        _ => execute_remote_http_tool(descriptor.name.as_str(), &entry.backend, &invocation.input),
    }
}

fn execute_remote_http_tool(
    tool_name: &str,
    backend: &ToolBackendConfig,
    payload: &Value,
) -> Result<String, String> {
    let ToolBackendConfig::RemoteHttp {
        url,
        method,
        timeout_ms,
        headers,
    } = backend
    else {
        return Err(format!(
            "SysCall Error: Tool '{}' is registered but has no executor for backend '{:?}'.",
            tool_name,
            backend.kind()
        ));
    };

    let endpoint = HttpEndpoint::parse(url).map_err(|e| {
        format!(
            "SysCall Error: Remote tool '{}' has invalid endpoint: {}",
            tool_name, e
        )
    })?;

    enforce_remote_http_policy(tool_name, &endpoint)?;

    let path = if endpoint.base_path.is_empty() {
        "/"
    } else {
        endpoint.base_path.as_str()
    };

    let response = endpoint
        .request_json_with_options(
            &method.to_ascii_uppercase(),
            path,
            Some(payload),
            HttpRequestOptions {
                timeout_ms: *timeout_ms,
                max_request_bytes: remote_http_max_request_bytes(),
                max_response_bytes: remote_http_max_response_bytes(),
                extra_headers: Some(headers),
            },
        )
        .map_err(|e| {
            format!(
                "SysCall Error: Remote tool '{}' request failed: {}",
                tool_name, e
            )
        })?;

    if !(200..300).contains(&response.status_code) {
        return Err(format!(
            "SysCall Error: Remote tool '{}' returned HTTP {} ({}). {}",
            tool_name,
            response.status_code,
            response.status_line,
            summarize_remote_error_body(&response.body)
        ));
    }

    if let Some(output) = response
        .json
        .as_ref()
        .and_then(|json| json.get("output"))
        .and_then(Value::as_str)
    {
        return Ok(output.to_string());
    }

    if let Some(json) = response.json {
        return Ok(json.to_string());
    }

    if response.body.trim().is_empty() {
        Ok(format!(
            "Remote tool '{}' completed with empty response.",
            tool_name
        ))
    } else {
        Ok(response.body)
    }
}

fn enforce_remote_http_policy(tool_name: &str, endpoint: &HttpEndpoint) -> Result<(), String> {
    let allowed_hosts = remote_http_allowed_hosts();
    let host = endpoint.host.trim().to_ascii_lowercase();
    if !allowed_hosts.iter().any(|allowed| allowed == &host) {
        return Err(format!(
            "Remote tool '{}' endpoint host '{}' is not allowlisted.",
            tool_name, endpoint.host
        ));
    }

    let addr = format!("{}:{}", endpoint.host, endpoint.port);
    let resolved: Vec<_> = addr
        .to_socket_addrs()
        .map_err(|e| {
            format!(
                "Remote tool '{}' failed to resolve '{}': {}",
                tool_name, addr, e
            )
        })?
        .collect();

    if resolved.is_empty() {
        return Err(format!(
            "Remote tool '{}' failed to resolve endpoint '{}'.",
            tool_name, addr
        ));
    }

    let declared_ip = endpoint.host.parse::<IpAddr>().ok();
    for socket_addr in resolved {
        let resolved_ip = socket_addr.ip();
        if is_disallowed_remote_ip(resolved_ip) && declared_ip != Some(resolved_ip) {
            return Err(format!(
                "Remote tool '{}' resolved host '{}' to disallowed address '{}'. Use an explicitly declared literal IP if this endpoint is intentional.",
                tool_name,
                endpoint.host,
                resolved_ip
            ));
        }
    }

    Ok(())
}

fn is_disallowed_remote_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(addr) => {
            addr.is_private()
                || addr.is_loopback()
                || addr.is_link_local()
                || addr.is_broadcast()
                || addr.is_documentation()
                || addr.is_multicast()
                || addr.is_unspecified()
        }
        IpAddr::V6(addr) => {
            addr.is_loopback()
                || addr.is_unspecified()
                || addr.is_unique_local()
                || addr.is_unicast_link_local()
                || addr.is_multicast()
        }
    }
}

fn remote_http_allowed_hosts() -> Vec<String> {
    if let Ok(value) = std::env::var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS") {
        return value
            .split(',')
            .map(|item| item.trim().to_ascii_lowercase())
            .filter(|item| !item.is_empty())
            .collect();
    }

    kernel_config().tools.remote_http_allowed_hosts.clone()
}

fn remote_http_max_request_bytes() -> usize {
    std::env::var("AGENTIC_REMOTE_TOOL_MAX_REQUEST_BYTES")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(|value| value.max(256))
        .unwrap_or(kernel_config().tools.remote_http_max_request_bytes)
}

fn remote_http_max_response_bytes() -> usize {
    std::env::var("AGENTIC_REMOTE_TOOL_MAX_RESPONSE_BYTES")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(|value| value.max(256))
        .unwrap_or(kernel_config().tools.remote_http_max_response_bytes)
}

fn summarize_remote_error_body(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        "Remote endpoint returned an empty error body.".to_string()
    } else {
        let mut snippet = trimmed.chars().take(200).collect::<String>();
        if trimmed.chars().count() > 200 {
            snippet.push_str("...");
        }
        format!("Remote body: {}", snippet)
    }
}

fn required_str<'a>(payload: &'a Value, key: &str) -> Result<&'a str, String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("SysCall Error: Missing required string field '{}'.", key))
}

fn handle_write_file_json(path: &str, content: &str) -> Result<String, String> {
    let mut args = String::with_capacity(path.len() + content.len() + 3);
    args.push_str(path);
    args.push_str(" | ");
    args.push_str(content);
    handle_write_file(&args)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

