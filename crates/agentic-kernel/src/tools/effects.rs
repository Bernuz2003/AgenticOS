use serde_json::{json, Value};

use crate::tool_registry::ToolRegistry;
use crate::tools::error::ToolError;
use crate::tools::invocation::ToolInvocation;

pub(crate) fn summarize_tool_effects(
    invocation: &ToolInvocation,
    output: Option<&Value>,
    registry: &ToolRegistry,
) -> Vec<Value> {
    let mut effects = match invocation.name.as_str() {
        "write_file" => vec![workspace_write_effect(
            "write_file",
            &invocation.input,
            output,
        )],
        "append_file" => vec![workspace_write_effect(
            "append_file",
            &invocation.input,
            output,
        )],
        "replace_in_file" => vec![workspace_write_effect(
            "replace_in_file",
            &invocation.input,
            output,
        )],
        "mkdir" => vec![workspace_directory_effect(&invocation.input, output)],
        "python" => vec![json!({
            "kind": "sandbox_execution",
            "tool_name": invocation.name,
            "workspace_effects": "unknown",
        })],
        _ => Vec::new(),
    };

    if effects.is_empty()
        && registry
            .get(&invocation.name)
            .is_some_and(|entry| entry.descriptor.dangerous)
    {
        effects.push(json!({
            "kind": "potential_side_effect",
            "tool_name": invocation.name,
            "workspace_effects": "unknown",
        }));
    }

    effects
}

pub(crate) fn tool_error_kind(error: &ToolError) -> &'static str {
    match error {
        ToolError::MalformedInvocation(_) => "malformed_invocation",
        ToolError::NotFound(_) => "not_found",
        ToolError::Disabled(_) => "disabled",
        ToolError::InvalidInput(_, _) => "invalid_input",
        ToolError::SchemaViolation(_, _) => "schema_violation",
        ToolError::OutputSchemaViolation(_, _) => "output_schema_violation",
        ToolError::PolicyDenied(_, _) => "policy_denied",
        ToolError::RateLimited(_) => "rate_limited",
        ToolError::Timeout(_, _) => "timeout",
        ToolError::BackendUnavailable(_, _) => "backend_unavailable",
        ToolError::ExecutionFailed(_, _) => "execution_failed",
        ToolError::Internal(_) => "internal",
    }
}

fn workspace_write_effect(tool_name: &str, input: &Value, output: Option<&Value>) -> Value {
    json!({
        "kind": "workspace_write",
        "tool_name": tool_name,
        "path": output
            .and_then(|payload| payload.get("path"))
            .and_then(Value::as_str)
            .or_else(|| input.get("path").and_then(Value::as_str)),
        "bytes_written": output
            .and_then(|payload| payload.get("bytes_written"))
            .or_else(|| output.and_then(|payload| payload.get("bytes_appended"))),
        "created": output
            .and_then(|payload| payload.get("created"))
            .and_then(Value::as_bool),
    })
}

fn workspace_directory_effect(input: &Value, output: Option<&Value>) -> Value {
    json!({
        "kind": "workspace_directory",
        "tool_name": "mkdir",
        "path": output
            .and_then(|payload| payload.get("path"))
            .and_then(Value::as_str)
            .or_else(|| input.get("path").and_then(Value::as_str)),
        "created": output
            .and_then(|payload| payload.get("created"))
            .and_then(Value::as_bool),
    })
}
