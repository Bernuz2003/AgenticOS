use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCaller {
    AgentText,
    Programmatic,
    ControlPlane,
}

impl ToolCaller {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AgentText => "agent_text",
            Self::Programmatic => "programmatic",
            Self::ControlPlane => "control_plane",
        }
    }
}

impl fmt::Display for ToolCaller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    pub pid: Option<u64>,
    pub session_id: Option<String>,
    pub caller: ToolCaller,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub name: String,
    pub input: serde_json::Value,
    pub call_id: Option<String>,
}
