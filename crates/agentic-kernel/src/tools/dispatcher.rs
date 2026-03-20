use crate::tool_registry::{HostExecutor, ToolBackendConfig, ToolRegistry};
use crate::tools::api::{Tool, ToolResult};
use crate::tools::error::ToolError;
use crate::tools::invocation::{ToolContext, ToolInvocation};
use crate::tools::schema::validate_value;
use std::collections::HashMap;

pub struct ToolDispatcher {
    builtins: HashMap<HostExecutor, Box<dyn Tool>>,
}

impl ToolDispatcher {
    pub fn new() -> Self {
        Self {
            builtins: crate::tools::builtins::host_builtin_dispatch_table(),
        }
    }

    pub fn dispatch(
        &self,
        invocation: &ToolInvocation,
        context: &ToolContext,
        registry: &ToolRegistry,
    ) -> Result<ToolResult, ToolError> {
        let entry = registry
            .resolve_invocation_name(&invocation.name)
            .ok_or_else(|| ToolError::NotFound(invocation.name.clone()))?;

        if !entry.descriptor.enabled {
            return Err(ToolError::Disabled(invocation.name.clone()));
        }

        if !entry
            .descriptor
            .allowed_callers
            .iter()
            .any(|caller| caller == &context.caller)
        {
            return Err(ToolError::PolicyDenied(
                invocation.name.clone(),
                format!("caller '{}' is not allowed", context.caller),
            ));
        }

        if !context.permissions.allows_tool(&invocation.name) {
            return Err(ToolError::PolicyDenied(
                invocation.name.clone(),
                format!(
                    "tool '{}' is outside the allowlist for trust scope '{}'",
                    invocation.name, context.permissions.trust_scope
                ),
            ));
        }

        if let Err(detail) = validate_value(
            &entry.descriptor.input_schema,
            &invocation.input,
            &format!("tool '{}'.input_schema", invocation.name),
        ) {
            return Err(ToolError::SchemaViolation(invocation.name.clone(), detail));
        }

        // Dispatch based on backend configuration
        let result = match &entry.backend {
            ToolBackendConfig::Host { executor } => {
                if let Some(tool) = self.builtins.get(executor) {
                    tool.execute(invocation, context)
                } else {
                    Err(ToolError::BackendUnavailable(
                        invocation.name.clone(),
                        format!("host executor '{executor:?}' is not available"),
                    ))
                }
            }
            ToolBackendConfig::Wasm { module, export } => Err(ToolError::BackendUnavailable(
                invocation.name.clone(),
                format!("Wasm support is not implemented yet (module: {module}, export: {export})"),
            )),
            ToolBackendConfig::RemoteHttp { .. } => {
                let tool = crate::tools::runner::RemoteHttpTool {
                    name: invocation.name.clone(),
                    backend: entry.backend.clone(),
                };
                tool.execute(invocation, context)
            }
        }?;

        if let Err(detail) = validate_value(
            &entry.descriptor.output_schema,
            &result.output,
            &format!("tool '{}'.output_schema", invocation.name),
        ) {
            return Err(ToolError::OutputSchemaViolation(
                invocation.name.clone(),
                detail,
            ));
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ToolDispatcher;
    use crate::tool_registry::{
        HostExecutor, ToolBackendConfig, ToolBackendKind, ToolDescriptor, ToolRegistry,
        ToolRegistryEntry, ToolSource,
    };
    use crate::tools::error::ToolError;
    use crate::tools::invocation::{
        ToolCaller, ToolContext, ToolInvocation, ToolInvocationTransport,
    };

    fn register_host_tool(
        registry: &mut ToolRegistry,
        name: &str,
        output_schema: serde_json::Value,
        allowed_callers: Vec<ToolCaller>,
    ) {
        registry
            .register(ToolRegistryEntry {
                descriptor: ToolDescriptor {
                    name: name.to_string(),
                    aliases: vec![],
                    description: "test host tool".to_string(),
                    input_schema: json!({
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }),
                    output_schema,
                    allowed_callers,
                    backend_kind: ToolBackendKind::Host,
                    capabilities: vec!["test".to_string()],
                    dangerous: false,
                    enabled: true,
                    source: ToolSource::BuiltIn,
                },
                backend: ToolBackendConfig::Host {
                    executor: HostExecutor::Dynamic("list_files".to_string()),
                },
            })
            .expect("register test tool");
    }

    #[test]
    fn rejects_disallowed_caller() {
        let mut registry = ToolRegistry::new();
        register_host_tool(
            &mut registry,
            "restricted_list",
            json!({
                "type": "object",
                "required": ["output", "entries"],
                "properties": {
                    "output": {"type": "string"},
                    "entries": {"type": "array", "items": {"type": "string"}}
                },
                "additionalProperties": false
            }),
            vec![ToolCaller::Programmatic],
        );
        let dispatcher = ToolDispatcher::new();
        let invocation = ToolInvocation {
            name: "restricted_list".to_string(),
            input: json!({}),
            call_id: None,
        };
        let context = ToolContext {
            pid: Some(1),
            session_id: None,
            caller: ToolCaller::AgentText,
            permissions: crate::tools::invocation::ProcessPermissionPolicy {
                trust_scope: crate::tools::invocation::ProcessTrustScope::InteractiveChat,
                actions_allowed: false,
                allowed_tools: vec!["restricted_list".to_string()],
                path_scopes: vec![".".to_string()],
            },
            transport: ToolInvocationTransport::Text,
            call_id: None,
        };

        let err = dispatcher
            .dispatch(&invocation, &context, &registry)
            .expect_err("caller denied");
        assert!(matches!(err, ToolError::PolicyDenied(_, _)));
    }

    #[test]
    fn validates_output_schema_after_execution() {
        let mut registry = ToolRegistry::new();
        register_host_tool(
            &mut registry,
            "bad_list_contract",
            json!({
                "type": "object",
                "required": ["count"],
                "properties": {
                    "count": {"type": "integer"}
                },
                "additionalProperties": false
            }),
            vec![ToolCaller::AgentText],
        );
        let dispatcher = ToolDispatcher::new();
        let invocation = ToolInvocation {
            name: "bad_list_contract".to_string(),
            input: json!({}),
            call_id: None,
        };
        let context = ToolContext {
            pid: Some(1),
            session_id: None,
            caller: ToolCaller::AgentText,
            permissions: crate::tools::invocation::ProcessPermissionPolicy {
                trust_scope: crate::tools::invocation::ProcessTrustScope::InteractiveChat,
                actions_allowed: false,
                allowed_tools: vec!["bad_list_contract".to_string()],
                path_scopes: vec![".".to_string()],
            },
            transport: ToolInvocationTransport::Text,
            call_id: None,
        };

        let err = dispatcher
            .dispatch(&invocation, &context, &registry)
            .expect_err("output schema violation");
        assert!(matches!(err, ToolError::OutputSchemaViolation(_, _)));
    }

    #[test]
    fn rejects_tool_outside_process_allowlist() {
        let mut registry = ToolRegistry::new();
        register_host_tool(
            &mut registry,
            "scoped_tool",
            json!({
                "type": "object",
                "required": ["output", "entries"],
                "properties": {
                    "output": {"type": "string"},
                    "entries": {"type": "array", "items": {"type": "string"}}
                },
                "additionalProperties": false
            }),
            vec![ToolCaller::AgentText],
        );
        let dispatcher = ToolDispatcher::new();
        let invocation = ToolInvocation {
            name: "scoped_tool".to_string(),
            input: json!({}),
            call_id: None,
        };
        let context = ToolContext {
            pid: Some(1),
            session_id: None,
            caller: ToolCaller::AgentText,
            permissions: crate::tools::invocation::ProcessPermissionPolicy {
                trust_scope: crate::tools::invocation::ProcessTrustScope::InteractiveChat,
                actions_allowed: false,
                allowed_tools: vec!["other_tool".to_string()],
                path_scopes: vec![".".to_string()],
            },
            transport: ToolInvocationTransport::Text,
            call_id: None,
        };

        let err = dispatcher
            .dispatch(&invocation, &context, &registry)
            .expect_err("allowlist denied");
        assert!(matches!(err, ToolError::PolicyDenied(_, _)));
    }
}
