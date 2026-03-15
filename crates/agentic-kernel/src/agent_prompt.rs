use crate::agent_capabilities::AgentCapabilityManifest;
use crate::tool_registry::ToolRegistry;
use crate::tools::invocation::ToolCaller;

pub(crate) fn build_agent_system_prompt(registry: &ToolRegistry, caller: ToolCaller) -> String {
    let manifest = crate::agent_capabilities::build_agent_capability_manifest(registry, caller);
    compose_agent_system_prompt(&manifest)
}

pub(crate) fn compose_agent_system_prompt(manifest: &AgentCapabilityManifest) -> String {
    let mut lines = vec![
        "You operate inside AgenticOS.".to_string(),
        "Use only canonical machine-readable invocations when you need kernel capabilities."
            .to_string(),
        format!("Tool syntax: {}.", manifest.tool_syntax),
        format!("Action syntax: {}.", manifest.action_syntax),
        "Rules:".to_string(),
        "- payloads must be single-line valid JSON objects".to_string(),
        "- emit {} even when there are no arguments".to_string(),
        "- never use legacy syntaxes like [[...]], PYTHON:, READ_FILE:, SPAWN or SEND".to_string(),
        "- if a tool or action is not listed below, do not invoke it".to_string(),
        "- TOOL calls use registered executors or resources; ACTION calls mutate the runtime/process graph".to_string(),
    ];

    lines.push("Available tools:".to_string());
    if manifest.tools.is_empty() {
        lines.push("- none".to_string());
    } else {
        for tool in &manifest.tools {
            let example =
                serde_json::to_string(&tool.input_example).unwrap_or_else(|_| "{}".to_string());
            let mut line = format!("- TOOL:{} {} : {}", tool.name, example, tool.description);
            if tool.dangerous {
                line.push_str(" Dangerous.");
            }
            lines.push(line);
        }
    }

    lines.push("Available actions:".to_string());
    if manifest.actions.is_empty() {
        lines.push("- none".to_string());
    } else {
        for action in &manifest.actions {
            let example =
                serde_json::to_string(&action.input_example).unwrap_or_else(|_| "{}".to_string());
            lines.push(format!(
                "- ACTION:{} {} : {}",
                action.name, example, action.description
            ));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::build_agent_system_prompt;
    use crate::tool_registry::ToolRegistry;
    use crate::tools::invocation::ToolCaller;

    #[test]
    fn system_prompt_lists_canonical_tools_and_actions() {
        let registry = ToolRegistry::with_builtins();
        let prompt = build_agent_system_prompt(&registry, ToolCaller::AgentText);

        assert!(prompt.contains("Tool syntax: TOOL:<name> <json-object>."));
        assert!(prompt.contains("Action syntax: ACTION:<name> <json-object>."));
        assert!(prompt.contains(r#"TOOL:calc {"expression":"string"}"#));
        assert!(prompt.contains(r#"ACTION:spawn {"prompt":"string"}"#));
        assert!(prompt.contains("never use legacy syntaxes"));
    }
}
