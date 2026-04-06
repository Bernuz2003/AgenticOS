use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::McpBridgeRuntime;
use crate::config::{KernelConfig, McpServerConfig, McpTransportConfig, McpTrustLevel};
use crate::tool_registry::ToolRegistry;
use crate::tools::dispatcher::ToolDispatcher;
use crate::tools::error::ToolError;
use crate::tools::invocation::{
    default_path_grants, PathGrantAccessMode, ProcessPathGrant, ProcessPermissionPolicy,
    ProcessTrustScope, ToolCaller, ToolContext, ToolInvocation, ToolInvocationTransport,
};

#[test]
fn registers_and_invokes_stdio_mcp_tool_through_internal_bridge() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).expect("create temp dir");
    let script_path = write_server_script(
        &temp_dir,
        "fake-mcp.sh",
        &standard_stdio_server_script(None),
    );

    let mut registry = ToolRegistry::with_builtins();
    let bridge = McpBridgeRuntime::start(
        &kernel_config_for_servers(vec![stdio_server_config(
            "demo",
            &script_path,
            &temp_dir,
            McpTrustLevel::Trusted,
            5_000,
        )]),
        &mut registry,
    )
    .expect("start mcp bridge")
    .expect("bridge enabled");

    assert!(
        registry.get("demo.echo").is_some(),
        "MCP-backed tool must be runtime-registered"
    );
    assert_eq!(bridge.registered_tool_names(), &["demo.echo".to_string()]);

    let result = dispatch_echo(
        &registry,
        tool_context(vec!["demo.echo".to_string()], default_path_grants()),
    )
    .expect("dispatch mcp tool");

    assert_eq!(
        result
            .output
            .get("output")
            .and_then(serde_json::Value::as_str),
        Some("hello from mcp")
    );
    assert_eq!(
        result
            .output
            .get("structured_content")
            .and_then(|value| value.get("echo"))
            .and_then(serde_json::Value::as_str),
        Some("hello from mcp")
    );
    assert_eq!(
        result
            .output
            .get("mcp")
            .and_then(|value| value.get("server_id"))
            .and_then(serde_json::Value::as_str),
        Some("demo")
    );
    assert_eq!(
        result
            .output
            .get("mcp")
            .and_then(|value| value.get("validation_passed"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert!(result
        .output
        .get("mcp")
        .and_then(|value| value.get("latency_ms"))
        .and_then(serde_json::Value::as_u64)
        .is_some());
    assert!(result
        .output
        .get("mcp")
        .and_then(|value| value.get("roots"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|roots| !roots.is_empty()));
}

#[test]
fn denies_mcp_tool_before_dispatch_when_process_policy_excludes_it() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).expect("create temp dir");
    let invoked_marker = temp_dir.join("invoked.marker");
    let script_path = write_server_script(
        &temp_dir,
        "deny-mcp.sh",
        &standard_stdio_server_script(Some(&invoked_marker)),
    );

    let mut registry = ToolRegistry::with_builtins();
    let _bridge = McpBridgeRuntime::start(
        &kernel_config_for_servers(vec![stdio_server_config(
            "demo",
            &script_path,
            &temp_dir,
            McpTrustLevel::Trusted,
            5_000,
        )]),
        &mut registry,
    )
    .expect("start mcp bridge")
    .expect("bridge enabled");

    let err = dispatch_echo(&registry, tool_context(Vec::new(), default_path_grants()))
        .expect_err("policy deny");
    assert!(matches!(err, ToolError::PolicyDenied(_, _)));
    assert!(
        !invoked_marker.exists(),
        "dispatcher must deny before any MCP transport call happens"
    );
}

#[test]
fn roots_follow_process_path_grants() {
    let temp_dir = unique_temp_dir();
    let allowed_dir = temp_dir.join("allowed");
    fs::create_dir_all(&allowed_dir).expect("create allowed dir");
    let script_path = write_server_script(
        &temp_dir,
        "roots-mcp.sh",
        &standard_stdio_server_script(None),
    );

    let mut registry = ToolRegistry::with_builtins();
    let _bridge = McpBridgeRuntime::start(
        &kernel_config_for_servers(vec![stdio_server_config(
            "demo",
            &script_path,
            &temp_dir,
            McpTrustLevel::Trusted,
            5_000,
        )]),
        &mut registry,
    )
    .expect("start mcp bridge")
    .expect("bridge enabled");

    let grants = vec![ProcessPathGrant {
        root: allowed_dir.to_string_lossy().to_string(),
        access_mode: PathGrantAccessMode::ReadOnly,
        capsule: Some("allowed".to_string()),
        label: Some("Allowed".to_string()),
    }];
    let result = dispatch_echo(
        &registry,
        tool_context(vec!["demo.echo".to_string()], grants),
    )
    .expect("dispatch mcp tool");

    let roots = result
        .output
        .get("mcp")
        .and_then(|value| value.get("roots"))
        .and_then(serde_json::Value::as_array)
        .expect("roots array");
    assert_eq!(roots.len(), 1);
    let expected_root = file_uri_for_path(&allowed_dir);
    assert_eq!(roots[0].as_str(), Some(expected_root.as_str()));
}

#[test]
fn replay_safe_policy_filters_dangerous_mcp_tools() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).expect("create temp dir");
    let script_path = write_server_script(
        &temp_dir,
        "replay-mcp.sh",
        &standard_stdio_server_script(None),
    );

    let mut registry = ToolRegistry::with_builtins();
    let _bridge = McpBridgeRuntime::start(
        &kernel_config_for_servers(vec![stdio_server_config(
            "demo",
            &script_path,
            &temp_dir,
            McpTrustLevel::Untrusted,
            5_000,
        )]),
        &mut registry,
    )
    .expect("start mcp bridge")
    .expect("bridge enabled");

    let entry = registry.get("demo.echo").expect("registered tool");
    assert!(entry.descriptor.dangerous);

    let base_context = tool_context(vec!["demo.echo".to_string()], default_path_grants());
    let replay_permissions = base_context.permissions.derive_replay_safe(&registry);
    assert!(replay_permissions.allowed_tools.is_empty());

    let err = dispatch_echo(
        &registry,
        ToolContext {
            permissions: replay_permissions,
            ..base_context
        },
    )
    .expect_err("replay-safe policy should deny dangerous MCP tool");
    assert!(matches!(err, ToolError::PolicyDenied(_, _)));
}

#[test]
fn reports_timeout_for_slow_mcp_server() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).expect("create temp dir");
    let script_path = write_server_script(&temp_dir, "slow-mcp.sh", &slow_stdio_server_script(1));

    let mut registry = ToolRegistry::with_builtins();
    let _bridge = McpBridgeRuntime::start(
        &kernel_config_for_servers(vec![stdio_server_config(
            "demo",
            &script_path,
            &temp_dir,
            McpTrustLevel::Trusted,
            100,
        )]),
        &mut registry,
    )
    .expect("start mcp bridge")
    .expect("bridge enabled");

    let err = dispatch_echo(
        &registry,
        tool_context(vec!["demo.echo".to_string()], default_path_grants()),
    )
    .expect_err("timeout expected");
    assert!(matches!(err, ToolError::Timeout(name, _) if name == "demo.echo"));
}

#[test]
fn timeout_cancels_inflight_mcp_server_work() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).expect("create temp dir");
    let timeout_marker = temp_dir.join("timeout-once.marker");
    let late_marker = temp_dir.join("late.marker");
    let script_path = write_server_script(
        &temp_dir,
        "recovering-mcp.sh",
        &recovering_slow_stdio_server_script(&timeout_marker, &late_marker),
    );

    let mut registry = ToolRegistry::with_builtins();
    let _bridge = McpBridgeRuntime::start(
        &kernel_config_for_servers(vec![stdio_server_config(
            "demo",
            &script_path,
            &temp_dir,
            McpTrustLevel::Trusted,
            100,
        )]),
        &mut registry,
    )
    .expect("start mcp bridge")
    .expect("bridge enabled");

    let first = dispatch_echo(
        &registry,
        tool_context(vec!["demo.echo".to_string()], default_path_grants()),
    );
    assert!(matches!(first, Err(ToolError::Timeout(name, _)) if name == "demo.echo"));

    std::thread::sleep(std::time::Duration::from_millis(1_200));
    assert!(
        !late_marker.exists(),
        "timed out MCP request should be cancelled before the server finishes late work"
    );
}

#[test]
fn keeps_healthy_mcp_tools_available_when_one_server_fails_discovery() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).expect("create temp dir");
    let good_script = write_server_script(
        &temp_dir,
        "good-mcp.sh",
        &standard_stdio_server_script(None),
    );
    let missing_script = temp_dir.join("missing-mcp.sh");

    let mut registry = ToolRegistry::with_builtins();
    let bridge = McpBridgeRuntime::start(
        &kernel_config_for_servers(vec![
            stdio_server_config(
                "good",
                &good_script,
                &temp_dir,
                McpTrustLevel::Trusted,
                5_000,
            ),
            stdio_server_config(
                "bad",
                &missing_script,
                &temp_dir,
                McpTrustLevel::Trusted,
                5_000,
            ),
        ]),
        &mut registry,
    )
    .expect("start mcp bridge")
    .expect("bridge enabled");

    assert_eq!(bridge.registered_tool_names(), &["good.echo".to_string()]);
    assert!(registry.get("good.echo").is_some());
    assert!(registry.get("bad.echo").is_none());
}

#[test]
fn skips_invalid_mcp_tool_schema_during_discovery() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).expect("create temp dir");
    let script_path = write_server_script(
        &temp_dir,
        "invalid-schema-mcp.sh",
        &invalid_schema_stdio_server_script(),
    );

    let mut registry = ToolRegistry::with_builtins();
    let bridge = McpBridgeRuntime::start(
        &kernel_config_for_servers(vec![stdio_server_config(
            "demo",
            &script_path,
            &temp_dir,
            McpTrustLevel::Trusted,
            5_000,
        )]),
        &mut registry,
    )
    .expect("start mcp bridge")
    .expect("bridge enabled");

    assert!(bridge.registered_tool_names().is_empty());
    assert!(registry.get("demo.echo").is_none());
}

#[test]
fn skips_invalid_mcp_output_schema_during_discovery() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).expect("create temp dir");
    let script_path = write_server_script(
        &temp_dir,
        "invalid-output-schema-mcp.sh",
        &invalid_output_schema_stdio_server_script(),
    );

    let mut registry = ToolRegistry::with_builtins();
    let bridge = McpBridgeRuntime::start(
        &kernel_config_for_servers(vec![stdio_server_config(
            "demo",
            &script_path,
            &temp_dir,
            McpTrustLevel::Trusted,
            5_000,
        )]),
        &mut registry,
    )
    .expect("start mcp bridge")
    .expect("bridge enabled");

    assert!(bridge.registered_tool_names().is_empty());
    assert!(registry.get("demo.echo").is_none());
}

#[test]
fn disabled_mcp_rollout_registers_nothing() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).expect("create temp dir");
    let script_path = write_server_script(
        &temp_dir,
        "disabled-mcp.sh",
        &standard_stdio_server_script(None),
    );

    let mut config = KernelConfig::default();
    config.mcp.enabled = false;
    config.mcp.servers = vec![stdio_server_config(
        "demo",
        &script_path,
        &temp_dir,
        McpTrustLevel::Trusted,
        5_000,
    )];

    let mut registry = ToolRegistry::with_builtins();
    let bridge = McpBridgeRuntime::start(&config, &mut registry).expect("bridge startup");

    assert!(bridge.is_none());
    assert!(registry.get("demo.echo").is_none());
}

fn dispatch_echo(
    registry: &ToolRegistry,
    context: ToolContext,
) -> Result<crate::tools::api::ToolResult, ToolError> {
    let dispatcher = ToolDispatcher::new();
    let invocation = ToolInvocation::new(
        "demo.echo",
        json!({"message":"hello"}),
        Some("call-1".to_string()),
    )
    .expect("invocation");
    dispatcher.dispatch(&invocation, &context, registry)
}

fn tool_context(allowed_tools: Vec<String>, path_grants: Vec<ProcessPathGrant>) -> ToolContext {
    ToolContext {
        pid: Some(7),
        session_id: Some("session-7".to_string()),
        caller: ToolCaller::AgentText,
        permissions: ProcessPermissionPolicy {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools,
            path_scopes: path_grants.iter().map(|grant| grant.root.clone()).collect(),
            path_grants,
        },
        transport: ToolInvocationTransport::Structured,
        call_id: Some("call-1".to_string()),
    }
}

fn kernel_config_for_servers(servers: Vec<McpServerConfig>) -> KernelConfig {
    let mut config = KernelConfig::default();
    config.mcp.enabled = true;
    config.mcp.servers = servers;
    config
}

fn stdio_server_config(
    id: &str,
    script_path: &Path,
    cwd: &Path,
    trust_level: McpTrustLevel,
    timeout_ms: u64,
) -> McpServerConfig {
    McpServerConfig {
        id: id.to_string(),
        label: Some(format!("{id} MCP")),
        enabled: true,
        tool_prefix: Some(id.to_string()),
        exposed_tools: vec!["echo".to_string()],
        default_allowlisted: false,
        approval_required: false,
        roots_enabled: true,
        trust_level,
        transport: McpTransportConfig::Stdio {
            command: script_path.to_string_lossy().to_string(),
            args: Vec::new(),
            cwd: Some(cwd.to_path_buf()),
            env: Default::default(),
            timeout_ms,
        },
    }
}

fn write_server_script(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let script_path = dir.join(name);
    fs::write(&script_path, contents).expect("write fake server");
    make_executable(&script_path);
    script_path
}

fn standard_stdio_server_script(invoked_marker: Option<&Path>) -> String {
    let touch_line = invoked_marker
        .map(|path| format!("      : > '{}'\n", path.display()))
        .unwrap_or_default();
    let template = r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{"listChanged":true},"prompts":{"listChanged":false},"resources":{"listChanged":false}},"serverInfo":{"name":"fake-mcp","version":"1.0.0"}}}'
      ;;
    *'"method":"notifications/initialized"'*)
      ;;
    *'"method":"tools/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"Echo via fake MCP","inputSchema":{"type":"object","required":["message"],"properties":{"message":{"type":"string"}},"additionalProperties":false},"outputSchema":{"type":"object","required":["echo"],"properties":{"echo":{"type":"string"}},"additionalProperties":false},"annotations":{"readOnlyHint":true,"idempotentHint":true}}]}}'
      ;;
    *'"method":"prompts/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"prompts":[]}}'
      ;;
    *'"method":"resources/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":4,"result":{"resources":[]}}'
      ;;
    *'"method":"notifications/roots/list_changed"'*)
      ;;
    *'"method":"tools/call"'*)
__TOUCH__
      printf '%s\n' '{"jsonrpc":"2.0","id":900,"method":"roots/list"}'
      IFS= read -r roots_line
      case "$roots_line" in
        *'"roots"'*'"file:///'*)
          printf '%s\n' '{"jsonrpc":"2.0","id":5,"result":{"content":[{"type":"text","text":"hello from mcp"}],"structuredContent":{"echo":"hello from mcp"},"isError":false}}'
          ;;
        *)
          printf '%s\n' '{"jsonrpc":"2.0","id":5,"result":{"content":[{"type":"text","text":"roots missing"}],"isError":true}}'
          ;;
      esac
      ;;
  esac
done
"#;
    template.replace("__TOUCH__\n", &touch_line)
}

fn slow_stdio_server_script(delay_s: u64) -> String {
    format!(
        r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25","capabilities":{{"tools":{{"listChanged":true}},"prompts":{{"listChanged":false}},"resources":{{"listChanged":false}}}},"serverInfo":{{"name":"slow-mcp","version":"1.0.0"}}}}}}'
      ;;
    *'"method":"notifications/initialized"'*)
      ;;
    *'"method":"tools/list"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"tools":[{{"name":"echo","description":"Slow echo","inputSchema":{{"type":"object","required":["message"],"properties":{{"message":{{"type":"string"}}}},"additionalProperties":false}},"outputSchema":{{"type":"object","required":["echo"],"properties":{{"echo":{{"type":"string"}}}},"additionalProperties":false}},"annotations":{{"readOnlyHint":true,"idempotentHint":true}}}}]}}}}'
      ;;
    *'"method":"prompts/list"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"prompts":[]}}}}'
      ;;
    *'"method":"resources/list"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":4,"result":{{"resources":[]}}}}'
      ;;
    *'"method":"notifications/roots/list_changed"'*)
      ;;
    *'"method":"tools/call"'*)
      sleep {delay_s}
      printf '%s\n' '{{"jsonrpc":"2.0","id":5,"result":{{"content":[{{"type":"text","text":"too late"}}],"structuredContent":{{"echo":"too late"}},"isError":false}}}}'
      ;;
  esac
done
"#
        ,
        delay_s = delay_s
    )
}

fn invalid_schema_stdio_server_script() -> String {
    r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{"listChanged":true},"prompts":{"listChanged":false},"resources":{"listChanged":false}},"serverInfo":{"name":"invalid-schema-mcp","version":"1.0.0"}}}'
      ;;
    *'"method":"notifications/initialized"'*)
      ;;
    *'"method":"tools/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"Invalid schema","inputSchema":{"type":7},"annotations":{"readOnlyHint":true}}]}}'
      ;;
    *'"method":"prompts/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"prompts":[]}}'
      ;;
    *'"method":"resources/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":4,"result":{"resources":[]}}'
      ;;
  esac
done
"#
    .to_string()
}

fn invalid_output_schema_stdio_server_script() -> String {
    r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{"listChanged":true},"prompts":{"listChanged":false},"resources":{"listChanged":false}},"serverInfo":{"name":"invalid-output-schema-mcp","version":"1.0.0"}}}'
      ;;
    *'"method":"notifications/initialized"'*)
      ;;
    *'"method":"tools/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"Invalid output schema","inputSchema":{"type":"object","required":["message"],"properties":{"message":{"type":"string"}},"additionalProperties":false},"outputSchema":{"type":7},"annotations":{"readOnlyHint":true}}]}}'
      ;;
    *'"method":"prompts/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"prompts":[]}}'
      ;;
    *'"method":"resources/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":4,"result":{"resources":[]}}'
      ;;
  esac
done
"#
    .to_string()
}

fn recovering_slow_stdio_server_script(timeout_marker: &Path, late_marker: &Path) -> String {
    format!(
        r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25","capabilities":{{"tools":{{"listChanged":true}},"prompts":{{"listChanged":false}},"resources":{{"listChanged":false}}}},"serverInfo":{{"name":"recovering-mcp","version":"1.0.0"}}}}}}'
      ;;
    *'"method":"notifications/initialized"'*)
      ;;
    *'"method":"tools/list"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"tools":[{{"name":"echo","description":"Recovering echo","inputSchema":{{"type":"object","required":["message"],"properties":{{"message":{{"type":"string"}}}},"additionalProperties":false}},"outputSchema":{{"type":"object","required":["echo"],"properties":{{"echo":{{"type":"string"}}}},"additionalProperties":false}},"annotations":{{"readOnlyHint":true,"idempotentHint":true}}}}]}}}}'
      ;;
    *'"method":"prompts/list"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"prompts":[]}}}}'
      ;;
    *'"method":"resources/list"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":4,"result":{{"resources":[]}}}}'
      ;;
    *'"method":"notifications/roots/list_changed"'*)
      ;;
    *'"method":"tools/call"'*)
      if [ -f '{timeout_marker}' ]; then
        printf '%s\n' '{{"jsonrpc":"2.0","id":5,"result":{{"content":[{{"type":"text","text":"recovered after timeout"}}],"structuredContent":{{"echo":"recovered after timeout"}},"isError":false}}}}'
      else
        : > '{timeout_marker}'
        sleep 1
        : > '{late_marker}'
        printf '%s\n' '{{"jsonrpc":"2.0","id":5,"result":{{"content":[{{"type":"text","text":"too late"}}],"structuredContent":{{"echo":"too late"}},"isError":false}}}}'
      fi
      ;;
  esac
done
"#
        ,
        timeout_marker = timeout_marker.display(),
        late_marker = late_marker.display()
    )
}

fn unique_temp_dir() -> PathBuf {
    let mut dir = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    dir.push(format!("agenticos-mcp-test-{nanos}"));
    dir
}

fn file_uri_for_path(path: &Path) -> String {
    let display = path.to_string_lossy().replace('\\', "/");
    let encoded = display
        .bytes()
        .flat_map(percent_encode_byte)
        .collect::<String>();
    format!("file://{encoded}")
}

fn percent_encode_byte(byte: u8) -> Vec<char> {
    if matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/') {
        return vec![byte as char];
    }

    format!("%{byte:02X}").chars().collect()
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::metadata(path).expect("metadata");
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("chmod");
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}
