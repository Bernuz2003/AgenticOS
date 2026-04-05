use std::fmt;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::tool_registry::ToolRegistry;
use crate::tools::error::ToolError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCaller {
    AgentText,
    AgentSupervisor,
    Programmatic,
    ControlPlane,
}

impl ToolCaller {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AgentText => "agent_text",
            Self::AgentSupervisor => "agent_supervisor",
            Self::Programmatic => "programmatic",
            Self::ControlPlane => "control_plane",
        }
    }

    pub fn can_orchestrate_actions(&self) -> bool {
        matches!(self, Self::AgentSupervisor | Self::ControlPlane)
    }
}

impl fmt::Display for ToolCaller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessTrustScope {
    InteractiveChat,
    WorkflowSupervisor,
    Programmatic,
    ControlPlane,
}

impl ProcessTrustScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InteractiveChat => "interactive_chat",
            Self::WorkflowSupervisor => "workflow_supervisor",
            Self::Programmatic => "programmatic",
            Self::ControlPlane => "control_plane",
        }
    }
}

impl fmt::Display for ProcessTrustScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessPermissionOverrides {
    #[serde(default)]
    pub trust_scope: Option<ProcessTrustScope>,
    #[serde(default)]
    pub allow_actions: Option<bool>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub path_scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessPermissionPolicy {
    pub trust_scope: ProcessTrustScope,
    pub actions_allowed: bool,
    pub allowed_tools: Vec<String>,
    pub path_scopes: Vec<String>,
}

impl ProcessPermissionPolicy {
    pub fn interactive_chat(registry: &ToolRegistry) -> Result<Self, String> {
        Self::build_for_caller(
            registry,
            &ToolCaller::AgentText,
            ProcessTrustScope::InteractiveChat,
            false,
            None,
        )
    }

    pub fn workflow_supervisor(
        registry: &ToolRegistry,
        overrides: Option<&ProcessPermissionOverrides>,
    ) -> Result<Self, String> {
        Self::build_for_caller(
            registry,
            &ToolCaller::AgentSupervisor,
            ProcessTrustScope::WorkflowSupervisor,
            true,
            overrides,
        )
    }

    #[allow(dead_code)]
    pub fn programmatic(registry: &ToolRegistry) -> Result<Self, String> {
        Self::build_for_caller(
            registry,
            &ToolCaller::Programmatic,
            ProcessTrustScope::Programmatic,
            false,
            None,
        )
    }

    #[allow(dead_code)]
    pub fn control_plane(registry: &ToolRegistry) -> Result<Self, String> {
        Self::build_for_caller(
            registry,
            &ToolCaller::ControlPlane,
            ProcessTrustScope::ControlPlane,
            true,
            None,
        )
    }

    pub fn build_for_caller(
        registry: &ToolRegistry,
        caller: &ToolCaller,
        default_trust_scope: ProcessTrustScope,
        default_actions_allowed: bool,
        overrides: Option<&ProcessPermissionOverrides>,
    ) -> Result<Self, String> {
        let trust_scope = overrides
            .and_then(|policy| policy.trust_scope.clone())
            .unwrap_or(default_trust_scope);
        let actions_allowed = overrides
            .and_then(|policy| policy.allow_actions)
            .unwrap_or(default_actions_allowed)
            && caller.can_orchestrate_actions();
        let allowed_tools = match overrides.and_then(|policy| policy.allowed_tools.clone()) {
            Some(tools) => normalize_tool_allowlist(tools)?,
            None => allowlisted_tools_for_caller(registry, caller),
        };
        let path_scopes = match overrides.and_then(|policy| policy.path_scopes.clone()) {
            Some(scopes) => normalize_path_scopes(scopes)?,
            None => default_path_scopes(),
        };

        Ok(Self {
            trust_scope,
            actions_allowed,
            allowed_tools,
            path_scopes,
        })
    }

    pub fn derive_chat_child(&self) -> Self {
        Self {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: self.allowed_tools.clone(),
            path_scopes: self.path_scopes.clone(),
        }
    }

    pub fn derive_replay_safe(&self, registry: &ToolRegistry) -> Self {
        let allowed_tools = self
            .allowed_tools
            .iter()
            .filter(|tool_name| {
                registry
                    .get(tool_name)
                    .is_some_and(|entry| entry.descriptor.enabled && !entry.descriptor.dangerous)
            })
            .cloned()
            .collect();

        Self {
            trust_scope: self.trust_scope.clone(),
            actions_allowed: false,
            allowed_tools,
            path_scopes: self.path_scopes.clone(),
        }
    }

    pub fn allows_tool(&self, tool_name: &str) -> bool {
        self.allowed_tools
            .iter()
            .any(|candidate| candidate == tool_name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolInvocationTransport {
    Text,
    Structured,
}

impl ToolInvocationTransport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Structured => "structured",
        }
    }
}

impl fmt::Display for ToolInvocationTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    pub pid: Option<u64>,
    pub session_id: Option<String>,
    pub caller: ToolCaller,
    pub permissions: ProcessPermissionPolicy,
    pub transport: ToolInvocationTransport,
    pub call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolInvocation {
    pub name: String,
    pub input: serde_json::Value,
    pub call_id: Option<String>,
}

impl ToolInvocation {
    pub fn new(
        name: impl Into<String>,
        input: serde_json::Value,
        call_id: Option<String>,
    ) -> Result<Self, ToolError> {
        if !input.is_object() {
            return Err(ToolError::MalformedInvocation(
                "Invocation payload must be a JSON object.".to_string(),
            ));
        }

        Ok(Self {
            name: normalize_tool_name(&name.into()).map_err(ToolError::MalformedInvocation)?,
            input,
            call_id,
        })
    }
}

pub(crate) fn normalize_tool_name(name: &str) -> Result<String, String> {
    let normalized = name.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err("Tool name cannot be empty.".to_string());
    }
    if normalized
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-' | '.'))
    {
        Ok(normalized)
    } else {
        Err(format!(
            "Invalid tool name '{}'. Allowed characters: a-z, 0-9, '_', '-', '.'.",
            name
        ))
    }
}

fn allowlisted_tools_for_caller(registry: &ToolRegistry, caller: &ToolCaller) -> Vec<String> {
    let mut names: Vec<String> = registry
        .list()
        .into_iter()
        .filter(|entry| entry.descriptor.enabled)
        .filter(|entry| {
            entry
                .descriptor
                .allowed_callers
                .iter()
                .any(|value| value == caller)
        })
        .map(|entry| entry.descriptor.name.clone())
        .collect();
    names.sort();
    names.dedup();
    names
}

fn normalize_tool_allowlist(raw: Vec<String>) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for name in raw {
        let canonical = normalize_tool_name(&name)?;
        if !normalized.contains(&canonical) {
            normalized.push(canonical);
        }
    }
    normalized.sort();
    Ok(normalized)
}

fn default_path_scopes() -> Vec<String> {
    vec![".".to_string()]
}

fn normalize_path_scopes(raw: Vec<String>) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for scope in raw {
        let canonical = normalize_path_scope(&scope)?;
        if !normalized.contains(&canonical) {
            normalized.push(canonical);
        }
    }
    Ok(normalized)
}

fn normalize_path_scope(scope: &str) -> Result<String, String> {
    let trimmed = scope.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Ok(".".to_string());
    }

    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        return Err(format!(
            "Path scope '{}' must be workspace-relative.",
            scope
        ));
    }

    let mut segments = Vec::new();
    for component in candidate.components() {
        match component {
            Component::Normal(segment) => segments.push(segment.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                if segments.pop().is_none() {
                    return Err(format!(
                        "Path scope '{}' escapes the workspace root.",
                        scope
                    ));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Path scope '{}' is invalid.", scope));
            }
        }
    }

    if segments.is_empty() {
        Ok(".".to_string())
    } else {
        Ok(segments.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{normalize_path_scope, ToolInvocation};

    #[test]
    fn structured_invocation_normalizes_mixed_case_names() {
        let invocation =
            ToolInvocation::new("Read_File", json!({"path":"notes.txt"}), None).expect("invoke");
        assert_eq!(invocation.name, "read_file");
    }

    #[test]
    fn normalizes_path_scopes_without_escaping_workspace() {
        assert_eq!(
            normalize_path_scope("./docs/../src").expect("normalized"),
            "src"
        );
        assert_eq!(normalize_path_scope(".").expect("root"), ".");
        assert!(normalize_path_scope("../etc").is_err());
        assert!(normalize_path_scope("/tmp").is_err());
    }
}
