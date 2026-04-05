use serde_json::json;

use crate::tool_registry::{HostExecutor, ToolBackendConfig, ToolRegistry};
use crate::tools::executor::{build_structured_invocation, execute_structured_invocation};
use crate::tools::invocation::{
    ProcessPermissionPolicy, ProcessTrustScope, ToolCaller, ToolContext, ToolInvocationTransport,
};

fn text_context() -> ToolContext {
    ToolContext {
        pid: Some(1),
        session_id: Some("system-tools".to_string()),
        caller: ToolCaller::AgentText,
        permissions: ProcessPermissionPolicy {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: vec!["get_time".to_string()],
            path_scopes: vec![".".to_string()],
        },
        transport: ToolInvocationTransport::Structured,
        call_id: None,
    }
}

#[test]
fn get_time_is_registered_as_dynamic_host_builtin() {
    let registry = ToolRegistry::with_builtins();
    let entry = registry.get("get_time").expect("get_time builtin");

    assert_eq!(
        entry.backend,
        ToolBackendConfig::Host {
            executor: HostExecutor::Dynamic("get_time".to_string())
        }
    );
}

#[test]
fn get_time_returns_time_fields() {
    let registry = ToolRegistry::with_builtins();
    let execution = execute_structured_invocation(
        build_structured_invocation("get_time", json!({}), None).expect("invocation"),
        &text_context(),
        &registry,
    )
    .expect("get_time executes");

    assert!(execution.result.output["unix_timestamp_ms"].is_number());
    assert!(execution.result.output["datetime_local"].is_string());
    assert!(execution.result.output["datetime_utc"].is_string());
    assert!(execution.result.output["date"].is_string());
    assert!(execution.result.output["time"].is_string());
    assert!(execution.result.output["weekday"].is_string());
    assert!(execution.result.output["timezone_offset"].is_string());
    assert!(execution.result.display_text.is_some());
}
