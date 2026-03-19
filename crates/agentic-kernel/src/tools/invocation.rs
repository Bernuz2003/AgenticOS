use std::fmt;

use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ToolInvocation;

    #[test]
    fn structured_invocation_normalizes_mixed_case_names() {
        let invocation =
            ToolInvocation::new("Read_File", json!({"path":"notes.txt"}), None).expect("invoke");
        assert_eq!(invocation.name, "read_file");
    }
}
