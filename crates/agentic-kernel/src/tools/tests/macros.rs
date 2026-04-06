use std::collections::BTreeSet;
use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agentic_kernel_macros::agentic_tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::api::{typed_output_to_tool_result, Tool, ToolResult};
use super::error::ToolError;
use super::executor::{build_structured_invocation, execute_structured_invocation};
use super::invocation::{
    default_path_grants, ProcessPermissionPolicy, ProcessTrustScope, ToolCaller, ToolContext,
    ToolInvocation, ToolInvocationTransport,
};
use super::path_guard::workspace_root;
use crate::tool_registry::{
    HostExecutor, ToolBackendConfig, ToolBackendKind, ToolDescriptor, ToolRegistry,
    ToolRegistryEntry, ToolSource,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EchoInput {
    message: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct EchoOutput {
    output: String,
    length: usize,
}

#[agentic_tool(
    name = "echo_macro",
    description = "Echo the provided string.",
    capabilities = ["test", "echo"],
    allowed_callers = [AgentText, Programmatic]
)]
fn echo_macro(
    input: EchoInput,
    _ctx: &super::invocation::ToolContext,
) -> Result<EchoOutput, ToolError> {
    Ok(EchoOutput {
        output: input.message.clone(),
        length: input.message.len(),
    })
}

struct ManualEchoTool;

impl Tool for ManualEchoTool {
    fn name(&self) -> &str {
        "echo_macro"
    }

    fn execute(
        &self,
        invocation: &ToolInvocation,
        context: &super::invocation::ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let input: EchoInput = serde_json::from_value(invocation.input.clone()).map_err(|err| {
            ToolError::InvalidInput(
                self.name().to_string(),
                format!("failed to deserialize input: {err}"),
            )
        })?;
        let output = echo_macro(input, context)?;
        typed_output_to_tool_result(self.name(), output)
    }
}

fn manual_echo_registry_entry() -> ToolRegistryEntry {
    ToolRegistryEntry {
        descriptor: ToolDescriptor {
            name: "echo_macro".to_string(),
            aliases: vec![],
            description: "Echo the provided string.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["message"],
                "properties": {
                    "message": {"type": "string"}
                },
                "additionalProperties": false
            }),
            input_example: Some(json!({"message": "string"})),
            output_schema: json!({
                "type": "object",
                "required": ["output", "length"],
                "properties": {
                    "output": {"type": "string"},
                    "length": {"type": "integer", "minimum": 0}
                },
                "additionalProperties": false
            }),
            allowed_callers: vec![ToolCaller::AgentText, ToolCaller::Programmatic],
            backend_kind: ToolBackendKind::Host,
            capabilities: vec!["test".to_string(), "echo".to_string()],
            dangerous: false,
            enabled: true,
            default_allowlisted: true,
            approval_required: false,
            interop: None,
            source: ToolSource::BuiltIn,
        },
        backend: ToolBackendConfig::Host {
            executor: HostExecutor::Dynamic("echo_macro".to_string()),
        },
    }
}

fn text_context() -> ToolContext {
    ToolContext {
        pid: Some(1),
        session_id: Some("session-1".to_string()),
        caller: ToolCaller::AgentText,
        permissions: ProcessPermissionPolicy {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: vec![
                "echo_macro".to_string(),
                "custom_echo".to_string(),
                "get_time".to_string(),
                "python".to_string(),
                "read_file".to_string(),
                "write_file".to_string(),
            ],
            path_grants: default_path_grants(),
            path_scopes: vec![".".to_string()],
        },
        transport: ToolInvocationTransport::Text,
        call_id: None,
    }
}

fn required_fields(schema: &Value) -> BTreeSet<&str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

#[test]
fn host_backend_dynamic_executor_serializes_as_string() {
    let backend = ToolBackendConfig::Host {
        executor: HostExecutor::Dynamic("read_file".to_string()),
    };

    let serialized = serde_json::to_value(&backend).expect("serialize backend");
    assert_eq!(
        serialized,
        json!({
            "kind": "host",
            "executor": "read_file"
        })
    );

    let roundtrip: ToolBackendConfig =
        serde_json::from_value(serialized).expect("deserialize backend");
    assert_eq!(roundtrip, backend);
}

#[test]
fn macro_generated_descriptor_uses_semantically_equivalent_schema() {
    let generated = echo_macro_registry_entry();
    let manual = manual_echo_registry_entry();

    assert_eq!(generated.descriptor.name, manual.descriptor.name);
    assert_eq!(
        generated.descriptor.description,
        manual.descriptor.description
    );
    assert_eq!(
        generated.descriptor.allowed_callers,
        manual.descriptor.allowed_callers
    );
    assert_eq!(
        generated.descriptor.capabilities,
        manual.descriptor.capabilities
    );
    assert_eq!(
        generated.descriptor.backend_kind,
        manual.descriptor.backend_kind
    );
    assert_eq!(generated.backend, manual.backend);

    assert_eq!(generated.descriptor.input_schema["type"], json!("object"));
    assert_eq!(
        generated.descriptor.input_schema["additionalProperties"],
        json!(false)
    );
    assert_eq!(
        required_fields(&generated.descriptor.input_schema),
        required_fields(&manual.descriptor.input_schema)
    );
    assert_eq!(
        generated.descriptor.input_schema["properties"]["message"]["type"],
        json!("string")
    );

    assert_eq!(generated.descriptor.output_schema["type"], json!("object"));
    assert_eq!(
        generated.descriptor.output_schema["additionalProperties"],
        json!(false)
    );
    assert_eq!(
        required_fields(&generated.descriptor.output_schema),
        required_fields(&manual.descriptor.output_schema)
    );
    assert_eq!(
        generated.descriptor.output_schema["properties"]["output"]["type"],
        json!("string")
    );
    assert_eq!(
        generated.descriptor.output_schema["properties"]["length"]["type"],
        json!("integer")
    );
}

#[test]
fn macro_generated_tool_matches_hand_written_glue() {
    let invocation =
        ToolInvocation::new("echo_macro", json!({"message":"ciao"}), None).expect("invocation");
    let context = text_context();

    let generated = EchoMacroTool
        .execute(&invocation, &context)
        .expect("macro tool executes");
    let manual = ManualEchoTool
        .execute(&invocation, &context)
        .expect("manual tool executes");

    assert_eq!(generated.output, manual.output);
    assert_eq!(generated.display_text, manual.display_text);
    assert_eq!(generated.warnings, manual.warnings);
}

#[test]
fn macro_generated_builtins_register_and_dispatch_without_text_parser_coupling() {
    let registry = ToolRegistry::with_builtins();
    let get_time = registry.get("get_time").expect("get_time builtin");
    let python = registry.get("python").expect("python builtin");
    let read_file = registry.get("read_file").expect("read_file builtin");
    let list_files = registry.get("list_files").expect("list_files builtin");
    let calc = registry.get("calc").expect("calc builtin");
    let write_file = registry.get("write_file").expect("write_file builtin");

    assert_eq!(
        get_time.backend,
        ToolBackendConfig::Host {
            executor: HostExecutor::Dynamic("get_time".to_string())
        }
    );
    assert_eq!(
        python.backend,
        ToolBackendConfig::Host {
            executor: HostExecutor::Dynamic("python".to_string())
        }
    );
    assert_eq!(
        read_file.backend,
        ToolBackendConfig::Host {
            executor: HostExecutor::Dynamic("read_file".to_string())
        }
    );
    assert_eq!(
        list_files.backend,
        ToolBackendConfig::Host {
            executor: HostExecutor::Dynamic("list_files".to_string())
        }
    );
    assert_eq!(
        calc.backend,
        ToolBackendConfig::Host {
            executor: HostExecutor::Dynamic("calc".to_string())
        }
    );
    assert_eq!(
        write_file.backend,
        ToolBackendConfig::Host {
            executor: HostExecutor::Dynamic("write_file".to_string())
        }
    );
}

#[test]
fn macro_generated_read_file_preserves_output_and_display_text() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_nanos();
    let relative_path = format!("macro_tool_test_{unique}.txt");
    let absolute_path = workspace_root()
        .expect("workspace root")
        .join(&relative_path);
    fs::write(&absolute_path, "macro mvp").expect("write fixture file");

    let registry = ToolRegistry::with_builtins();
    let execution = execute_structured_invocation(
        build_structured_invocation("read_file", json!({ "path": relative_path }), None)
            .expect("structured invocation"),
        &ToolContext {
            transport: ToolInvocationTransport::Structured,
            ..text_context()
        },
        &registry,
    )
    .expect("read_file executes");

    let _ = fs::remove_file(&absolute_path);

    assert_eq!(
        execution.result.output,
        json!({
            "output": "macro mvp",
            "path": execution.invocation.input["path"].as_str().expect("path string")
        })
    );
    assert_eq!(execution.result.display_text.as_deref(), Some("macro mvp"));
}

#[test]
fn macro_generated_write_file_preserves_output_shape() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_nanos();
    let relative_path = format!("macro_write_tool_test_{unique}.txt");
    let absolute_path = workspace_root()
        .expect("workspace root")
        .join(&relative_path);

    let registry = ToolRegistry::with_builtins();
    let execution = execute_structured_invocation(
        build_structured_invocation(
            "write_file",
            json!({ "path": relative_path, "content": "macro write" }),
            None,
        )
        .expect("structured invocation"),
        &ToolContext {
            transport: ToolInvocationTransport::Structured,
            ..text_context()
        },
        &registry,
    )
    .expect("write_file executes");

    assert_eq!(
        fs::read_to_string(&absolute_path).expect("written file"),
        "macro write"
    );
    assert_eq!(
        execution.result.output["path"],
        execution.invocation.input["path"]
    );
    assert_eq!(execution.result.output["bytes_written"], json!(11));
    assert!(execution
        .result
        .display_text
        .as_deref()
        .unwrap_or("")
        .contains("written"));

    let _ = fs::remove_file(&absolute_path);
}

// ---- ToolResult passthrough tests ----

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CustomEchoInput {
    message: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct CustomEchoOutputSchema {
    echoed: String,
}

#[agentic_tool(
    name = "custom_echo",
    description = "Echo with custom display_text and warnings.",
    capabilities = ["test"],
    allowed_callers = [AgentText, Programmatic],
    output_schema_type = CustomEchoOutputSchema
)]
fn custom_echo(
    input: CustomEchoInput,
    _ctx: &super::invocation::ToolContext,
) -> Result<ToolResult, ToolError> {
    Ok(ToolResult {
        output: json!({ "echoed": input.message }),
        display_text: Some(format!("Custom: {}", input.message)),
        warnings: vec!["this is a test warning".to_string()],
    })
}

#[test]
fn tool_result_passthrough_preserves_display_text_and_warnings() {
    let invocation =
        ToolInvocation::new("custom_echo", json!({"message":"ciao"}), None).expect("invocation");
    let context = text_context();

    let result = CustomEchoTool
        .execute(&invocation, &context)
        .expect("custom_echo executes");

    assert_eq!(result.output, json!({ "echoed": "ciao" }));
    assert_eq!(result.display_text.as_deref(), Some("Custom: ciao"));
    assert_eq!(result.warnings, vec!["this is a test warning"]);
}

#[test]
fn tool_result_passthrough_registry_entry_can_use_explicit_output_schema_type() {
    let entry = custom_echo_registry_entry();

    assert_eq!(entry.descriptor.name, "custom_echo");
    assert_eq!(entry.descriptor.output_schema["type"], json!("object"));
    assert_eq!(
        required_fields(&entry.descriptor.output_schema),
        BTreeSet::from(["echoed"])
    );
    assert_eq!(
        entry.descriptor.output_schema["properties"]["echoed"]["type"],
        json!("string")
    );
    assert_eq!(
        entry.descriptor.output_schema["additionalProperties"],
        json!(false)
    );
}
