use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::runtime::actions::builtin_action_descriptors;
use crate::tool_registry::{ToolBackendKind, ToolRegistry, ToolSource};
use crate::tools::invocation::ToolCaller;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct AgentCapabilityManifest {
    pub(crate) caller: ToolCaller,
    pub(crate) tool_syntax: String,
    pub(crate) action_syntax: String,
    pub(crate) tools: Vec<AgentToolManifestEntry>,
    pub(crate) actions: Vec<AgentActionManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct AgentToolManifestEntry {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema: Value,
    pub(crate) input_example: Value,
    pub(crate) dangerous: bool,
    pub(crate) backend_kind: ToolBackendKind,
    pub(crate) source: ToolSource,
    pub(crate) capabilities: Vec<String>,
    pub(crate) notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct AgentActionManifestEntry {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema: Value,
    pub(crate) input_example: Value,
    pub(crate) notes: Vec<String>,
}

pub(crate) fn build_agent_capability_manifest(
    registry: &ToolRegistry,
    caller: ToolCaller,
) -> AgentCapabilityManifest {
    let tools = registry
        .list()
        .into_iter()
        .filter(|entry| entry.descriptor.enabled)
        .filter(|entry| entry.descriptor.allowed_callers.contains(&caller))
        .map(|entry| AgentToolManifestEntry {
            name: entry.descriptor.name.clone(),
            description: entry.descriptor.description.clone(),
            input_schema: entry.descriptor.input_schema.clone(),
            input_example: synthesize_object_example(&entry.descriptor.input_schema),
            dangerous: entry.descriptor.dangerous,
            backend_kind: entry.descriptor.backend_kind.clone(),
            source: entry.descriptor.source.clone(),
            capabilities: entry.descriptor.capabilities.clone(),
            notes: build_tool_notes(entry.descriptor.dangerous, &entry.descriptor.capabilities),
        })
        .collect();

    let actions = if caller == ToolCaller::AgentText {
        builtin_action_descriptors()
            .into_iter()
            .map(|descriptor| AgentActionManifestEntry {
                name: descriptor.name,
                description: descriptor.description,
                input_schema: descriptor.input_schema.clone(),
                input_example: descriptor.input_example,
                notes: descriptor.notes,
            })
            .collect()
    } else {
        Vec::new()
    };

    AgentCapabilityManifest {
        caller,
        tool_syntax: "TOOL:<name> <json-object>".to_string(),
        action_syntax: "ACTION:<name> <json-object>".to_string(),
        tools,
        actions,
    }
}

fn build_tool_notes(dangerous: bool, capabilities: &[String]) -> Vec<String> {
    let mut notes = Vec::new();
    if dangerous {
        notes.push("Dangerous: this operation can execute code or mutate workspace state.".into());
    }
    if !capabilities.is_empty() {
        notes.push(format!("Capabilities: {}.", capabilities.join(", ")));
    }
    notes
}

fn synthesize_object_example(schema: &Value) -> Value {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return json!({});
    };

    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();

    let mut example = Map::new();
    if required.is_empty() {
        return Value::Object(example);
    }

    for field in required {
        let Some(field_schema) = properties.get(field) else {
            continue;
        };
        example.insert(field.to_string(), synthesize_value(field_schema));
    }

    Value::Object(example)
}

fn synthesize_value(schema: &Value) -> Value {
    match schema.get("type").and_then(Value::as_str) {
        Some("string") => Value::String("string".to_string()),
        Some("integer") => json!(0),
        Some("number") => json!(0),
        Some("boolean") => json!(false),
        Some("array") => Value::Array(Vec::new()),
        Some("object") => synthesize_object_example(schema),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::build_agent_capability_manifest;
    use crate::tool_registry::{
        HostExecutor, ToolBackendConfig, ToolBackendKind, ToolDescriptor, ToolRegistry,
        ToolRegistryEntry, ToolSource,
    };
    use crate::tools::invocation::ToolCaller;

    #[test]
    fn filters_disabled_and_disallowed_tools_from_manifest() {
        let mut registry = ToolRegistry::new();
        registry
            .register(ToolRegistryEntry {
                descriptor: ToolDescriptor {
                    name: "visible_tool".to_string(),
                    aliases: vec![],
                    description: "visible".to_string(),
                    input_schema: json!({
                        "type": "object",
                        "required": ["path"],
                        "properties": {"path": {"type": "string"}},
                        "additionalProperties": false
                    }),
                    output_schema: json!({"type": "object"}),
                    allowed_callers: vec![ToolCaller::AgentText],
                    backend_kind: ToolBackendKind::Host,
                    capabilities: vec!["fs".to_string()],
                    dangerous: false,
                    enabled: true,
                    source: ToolSource::BuiltIn,
                },
                backend: ToolBackendConfig::Host {
                    executor: HostExecutor::ReadFile,
                },
            })
            .expect("register visible tool");
        registry
            .register(ToolRegistryEntry {
                descriptor: ToolDescriptor {
                    name: "hidden_tool".to_string(),
                    aliases: vec![],
                    description: "hidden".to_string(),
                    input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
                    output_schema: json!({"type": "object"}),
                    allowed_callers: vec![ToolCaller::Programmatic],
                    backend_kind: ToolBackendKind::Host,
                    capabilities: vec![],
                    dangerous: false,
                    enabled: true,
                    source: ToolSource::BuiltIn,
                },
                backend: ToolBackendConfig::Host {
                    executor: HostExecutor::ListFiles,
                },
            })
            .expect("register hidden tool");
        registry
            .register(ToolRegistryEntry {
                descriptor: ToolDescriptor {
                    name: "disabled_tool".to_string(),
                    aliases: vec![],
                    description: "disabled".to_string(),
                    input_schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
                    output_schema: json!({"type": "object"}),
                    allowed_callers: vec![ToolCaller::AgentText],
                    backend_kind: ToolBackendKind::Host,
                    capabilities: vec![],
                    dangerous: false,
                    enabled: false,
                    source: ToolSource::BuiltIn,
                },
                backend: ToolBackendConfig::Host {
                    executor: HostExecutor::ListFiles,
                },
            })
            .expect("register disabled tool");

        let manifest = build_agent_capability_manifest(&registry, ToolCaller::AgentText);
        let tool_names: Vec<&str> = manifest
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();

        assert_eq!(tool_names, vec!["visible_tool"]);
        assert_eq!(manifest.tools[0].input_example, json!({"path": "string"}));
    }

    #[test]
    fn exposes_actions_only_to_agent_text_callers() {
        let registry = ToolRegistry::with_builtins();

        let agent_text_manifest = build_agent_capability_manifest(&registry, ToolCaller::AgentText);
        let programmatic_manifest =
            build_agent_capability_manifest(&registry, ToolCaller::Programmatic);

        assert_eq!(agent_text_manifest.actions.len(), 2);
        assert!(programmatic_manifest.actions.is_empty());
    }
}
