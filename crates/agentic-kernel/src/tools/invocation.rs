use std::fmt;
use std::path::{Component, Path, PathBuf};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathGrantAccessMode {
    ReadOnly,
    WriteApproved,
    AutonomousWrite,
}

impl PathGrantAccessMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::WriteApproved => "write_approved",
            Self::AutonomousWrite => "autonomous_write",
        }
    }

    pub fn allows_write(&self) -> bool {
        matches!(self, Self::WriteApproved | Self::AutonomousWrite)
    }
}

impl fmt::Display for PathGrantAccessMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessPathGrant {
    pub root: String,
    #[serde(default = "default_path_grant_access_mode")]
    pub access_mode: PathGrantAccessMode,
    #[serde(default)]
    pub capsule: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

impl ProcessPathGrant {
    pub fn workspace_relative(&self) -> bool {
        !Path::new(&self.root).is_absolute()
    }

    pub fn allows_write(&self) -> bool {
        self.access_mode.allows_write()
    }
}

fn default_path_grant_access_mode() -> PathGrantAccessMode {
    PathGrantAccessMode::AutonomousWrite
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
    #[serde(default)]
    pub path_grants: Option<Vec<ProcessPathGrant>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessPermissionPolicy {
    pub trust_scope: ProcessTrustScope,
    pub actions_allowed: bool,
    pub allowed_tools: Vec<String>,
    pub path_grants: Vec<ProcessPathGrant>,
    pub path_scopes: Vec<String>,
}

impl ProcessPermissionPolicy {
    pub fn interactive_chat(registry: &ToolRegistry) -> Result<Self, String> {
        Self::interactive_chat_with_overrides(registry, None)
    }

    pub fn interactive_chat_with_overrides(
        registry: &ToolRegistry,
        overrides: Option<&ProcessPermissionOverrides>,
    ) -> Result<Self, String> {
        Self::build_for_caller(
            registry,
            &ToolCaller::AgentText,
            ProcessTrustScope::InteractiveChat,
            false,
            overrides,
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
        let mut allowed_tools = match overrides.and_then(|policy| policy.allowed_tools.clone()) {
            Some(tools) => normalize_tool_allowlist(tools)?,
            None => allowlisted_tools_for_caller(registry, caller),
        };
        let path_grants = match overrides.and_then(|policy| policy.path_grants.clone()) {
            Some(grants) => normalize_path_grants(grants)?,
            None => match overrides.and_then(|policy| policy.path_scopes.clone()) {
                Some(scopes) => path_grants_from_scopes(scopes)?,
                None => default_path_grants(),
            },
        };
        allowed_tools = filter_tools_for_path_grants(allowed_tools, &path_grants);
        let path_scopes = path_grants
            .iter()
            .map(|grant| grant.root.clone())
            .collect::<Vec<_>>();

        Ok(Self {
            trust_scope,
            actions_allowed,
            allowed_tools,
            path_grants,
            path_scopes,
        })
    }

    pub fn derive_chat_child(&self) -> Self {
        Self {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: self.allowed_tools.clone(),
            path_grants: self.path_grants.clone(),
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
            path_grants: self.path_grants.clone(),
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
        .filter(|entry| entry.descriptor.default_allowlisted)
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

fn filter_tools_for_path_grants(
    mut allowed_tools: Vec<String>,
    path_grants: &[ProcessPathGrant],
) -> Vec<String> {
    let has_external_grants = path_grants
        .iter()
        .any(|grant| grant.root != "." && Path::new(&grant.root).is_absolute());
    if !has_external_grants {
        return allowed_tools;
    }

    allowed_tools
        .retain(|tool_name| !matches!(tool_name.as_str(), "python" | "calc" | "exec_command"));
    allowed_tools
}

pub(crate) fn default_path_grants() -> Vec<ProcessPathGrant> {
    vec![ProcessPathGrant {
        root: ".".to_string(),
        access_mode: PathGrantAccessMode::AutonomousWrite,
        capsule: Some("workspace".to_string()),
        label: Some("Workspace".to_string()),
    }]
}

fn path_grants_from_scopes(raw: Vec<String>) -> Result<Vec<ProcessPathGrant>, String> {
    let scopes = normalize_path_scopes(raw)?;
    Ok(scopes
        .into_iter()
        .map(|root| ProcessPathGrant {
            root,
            access_mode: PathGrantAccessMode::AutonomousWrite,
            capsule: None,
            label: None,
        })
        .collect())
}

fn normalize_path_grants(raw: Vec<ProcessPathGrant>) -> Result<Vec<ProcessPathGrant>, String> {
    let mut normalized: Vec<ProcessPathGrant> = Vec::new();
    for grant in raw {
        let canonical_root = normalize_path_scope(&grant.root)?;
        let canonical_capsule = grant
            .capsule
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());
        let canonical_label = grant
            .label
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let candidate = ProcessPathGrant {
            root: canonical_root,
            access_mode: grant.access_mode,
            capsule: canonical_capsule,
            label: canonical_label,
        };

        if let Some(existing) = normalized
            .iter_mut()
            .find(|existing| existing.root == candidate.root)
        {
            if path_grant_rank(&candidate.access_mode) > path_grant_rank(&existing.access_mode) {
                existing.access_mode = candidate.access_mode.clone();
            }
            if existing.capsule.is_none() {
                existing.capsule = candidate.capsule.clone();
            }
            if existing.label.is_none() {
                existing.label = candidate.label.clone();
            }
            continue;
        }

        normalized.push(candidate);
    }

    Ok(normalized)
}

fn path_grant_rank(mode: &PathGrantAccessMode) -> u8 {
    match mode {
        PathGrantAccessMode::ReadOnly => 0,
        PathGrantAccessMode::WriteApproved => 1,
        PathGrantAccessMode::AutonomousWrite => 2,
    }
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
        return normalize_absolute_path_scope(candidate);
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

fn normalize_absolute_path_scope(candidate: &Path) -> Result<String, String> {
    let mut normalized = PathBuf::new();
    let mut saw_root = false;
    let mut depth = 0usize;

    for component in candidate.components() {
        match component {
            Component::Prefix(prefix) => {
                normalized.push(prefix.as_os_str());
            }
            Component::RootDir => {
                normalized.push(std::path::MAIN_SEPARATOR_STR);
                saw_root = true;
            }
            Component::Normal(segment) => {
                normalized.push(segment);
                depth += 1;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if depth == 0 || !normalized.pop() {
                    return Err(format!(
                        "Path scope '{}' escapes the filesystem root.",
                        candidate.display()
                    ));
                }
                depth -= 1;
            }
        }
    }

    if !saw_root && candidate.is_absolute() && normalized.as_os_str().is_empty() {
        return Err(format!("Path scope '{}' is invalid.", candidate.display()));
    }

    Ok(normalized.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::tool_registry::ToolRegistry;

    use super::{
        normalize_path_grants, normalize_path_scope, PathGrantAccessMode, ProcessPathGrant,
        ProcessPermissionOverrides, ProcessPermissionPolicy, ToolInvocation,
    };

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
        assert_eq!(
            normalize_path_scope("/tmp/../var/data").expect("absolute normalized"),
            "/var/data"
        );
    }

    #[test]
    fn normalizes_and_merges_path_grants() {
        let grants = normalize_path_grants(vec![
            ProcessPathGrant {
                root: ".".to_string(),
                access_mode: PathGrantAccessMode::ReadOnly,
                capsule: Some("workspace".to_string()),
                label: None,
            },
            ProcessPathGrant {
                root: "./".to_string(),
                access_mode: PathGrantAccessMode::AutonomousWrite,
                capsule: None,
                label: Some("Workspace".to_string()),
            },
        ])
        .expect("normalize grants");

        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].root, ".");
        assert_eq!(grants[0].access_mode, PathGrantAccessMode::AutonomousWrite);
        assert_eq!(grants[0].capsule.as_deref(), Some("workspace"));
        assert_eq!(grants[0].label.as_deref(), Some("Workspace"));
    }

    #[test]
    fn external_absolute_grants_disable_unconfined_host_tools() {
        let registry = ToolRegistry::with_builtins();
        let policy = ProcessPermissionPolicy::interactive_chat_with_overrides(
            &registry,
            Some(&ProcessPermissionOverrides {
                allowed_tools: Some(vec![
                    "read_file".to_string(),
                    "python".to_string(),
                    "calc".to_string(),
                    "exec_command".to_string(),
                ]),
                path_grants: Some(vec![ProcessPathGrant {
                    root: "/tmp/agenticos-external".to_string(),
                    access_mode: PathGrantAccessMode::AutonomousWrite,
                    capsule: Some("host_fs".to_string()),
                    label: Some("External".to_string()),
                }]),
                ..ProcessPermissionOverrides::default()
            }),
        )
        .expect("policy");

        assert!(policy.allows_tool("read_file"));
        assert!(!policy.allows_tool("python"));
        assert!(!policy.allows_tool("calc"));
        assert!(!policy.allows_tool("exec_command"));
    }
}
