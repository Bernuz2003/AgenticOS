use serde_json::Value;

use crate::tool_registry::ToolRegistry;

use super::api::ToolResult;
use super::dispatcher::ToolDispatcher;
use super::error::ToolError;
use super::invocation::{ToolContext, ToolInvocation};

#[derive(Debug, Clone)]
#[allow(dead_code)] // Intentionally kept as raw dispatch layer for future control-plane use.
pub(crate) struct ToolExecution {
    pub invocation: ToolInvocation,
    pub result: ToolResult,
}

pub(crate) fn build_structured_invocation(
    name: impl Into<String>,
    input: Value,
    call_id: Option<String>,
) -> Result<ToolInvocation, ToolError> {
    ToolInvocation::new(name, input, call_id)
}

#[allow(dead_code)] // Intentionally kept as raw dispatch layer for future control-plane use.
pub(crate) fn execute_structured_invocation(
    invocation: ToolInvocation,
    context: &ToolContext,
    registry: &ToolRegistry,
) -> Result<ToolExecution, ToolError> {
    let dispatcher = ToolDispatcher::new();
    let result = dispatcher.dispatch(&invocation, context, registry)?;
    Ok(ToolExecution { invocation, result })
}

#[allow(dead_code)] // Intentionally kept as raw dispatch layer for future control-plane use.
pub(crate) fn execute_text_invocation(
    text: &str,
    context: &ToolContext,
    registry: &ToolRegistry,
) -> Result<ToolExecution, ToolError> {
    let invocation = crate::tools::parser::parse_text_invocation(text)?;
    execute_structured_invocation(invocation, context, registry)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        build_structured_invocation, execute_structured_invocation, execute_text_invocation,
    };
    use crate::tool_registry::{
        HostExecutor, ToolBackendConfig, ToolBackendKind, ToolDescriptor, ToolRegistry,
        ToolRegistryEntry, ToolSource,
    };
    use crate::tools::error::ToolError;
    use crate::tools::invocation::{
        default_path_grants, ProcessPermissionPolicy, ProcessTrustScope, ToolCaller, ToolContext,
        ToolInvocationTransport,
    };

    fn text_context() -> ToolContext {
        ToolContext {
            pid: Some(1),
            session_id: Some("session-1".to_string()),
            caller: ToolCaller::AgentText,
            permissions: ProcessPermissionPolicy {
                trust_scope: ProcessTrustScope::InteractiveChat,
                actions_allowed: false,
                allowed_tools: vec![
                    "calc".to_string(),
                    "list_files".to_string(),
                    "restricted".to_string(),
                ],
                path_grants: default_path_grants(),
                path_scopes: vec![".".to_string()],
            },
            transport: ToolInvocationTransport::Text,
            call_id: None,
        }
    }

    #[test]
    fn text_and_structured_paths_return_equivalent_results() {
        let registry = ToolRegistry::with_builtins();
        let text_result = execute_text_invocation(
            r#"TOOL:calc {"expression":"1+1"}"#,
            &text_context(),
            &registry,
        )
        .expect("text invocation succeeds");
        let structured_result = execute_structured_invocation(
            build_structured_invocation("calc", json!({"expression":"1+1"}), None)
                .expect("structured invocation"),
            &ToolContext {
                transport: ToolInvocationTransport::Structured,
                ..text_context()
            },
            &registry,
        )
        .expect("structured invocation succeeds");

        assert_eq!(
            text_result.invocation.name,
            structured_result.invocation.name
        );
        assert_eq!(text_result.result.output, structured_result.result.output);
        assert_eq!(
            text_result.result.display_text,
            structured_result.result.display_text
        );
    }

    #[test]
    fn structured_invocation_enforces_object_payloads() {
        let err = build_structured_invocation("calc", json!(["1+1"]), None)
            .expect_err("non-object payload must fail");
        assert!(matches!(err, ToolError::MalformedInvocation(_)));
    }

    #[test]
    fn text_and_structured_paths_share_caller_enforcement() {
        let mut registry = ToolRegistry::new();
        registry
            .register(ToolRegistryEntry {
                descriptor: ToolDescriptor {
                    name: "restricted".to_string(),
                    aliases: vec![],
                    description: "restricted".to_string(),
                    input_schema: json!({
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }),
                    input_example: None,
                    output_schema: json!({
                        "type": "object",
                        "required": ["output", "entries"],
                        "properties": {
                            "output": {"type": "string"},
                            "entries": {"type": "array", "items": {"type": "string"}}
                        },
                        "additionalProperties": false
                    }),
                    allowed_callers: vec![ToolCaller::Programmatic],
                    backend_kind: ToolBackendKind::Host,
                    capabilities: vec![],
                    dangerous: false,
                    enabled: true,
                    source: ToolSource::BuiltIn,
                },
                backend: ToolBackendConfig::Host {
                    executor: HostExecutor::Dynamic("list_files".to_string()),
                },
            })
            .expect("register restricted tool");

        let text_err = execute_text_invocation(r#"TOOL:restricted {}"#, &text_context(), &registry)
            .expect_err("text caller denied");
        let structured_err = execute_structured_invocation(
            build_structured_invocation("restricted", json!({}), None).expect("structured build"),
            &ToolContext {
                transport: ToolInvocationTransport::Structured,
                ..text_context()
            },
            &registry,
        )
        .expect_err("structured caller denied");

        assert!(matches!(text_err, ToolError::PolicyDenied(_, _)));
        assert!(matches!(structured_err, ToolError::PolicyDenied(_, _)));
    }
}
