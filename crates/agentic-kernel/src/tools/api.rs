use crate::tools::error::ToolError;
use crate::tools::invocation::{ToolContext, ToolInvocation};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Strutturato per eventuale Programmatic Tool Calling
    pub output: serde_json::Value,
    /// Testo fallback primario da mostrare all'agente o in UI
    pub display_text: Option<String>,
    /// Avvisi non bloccanti formatisi durante l'esecuzione
    pub warnings: Vec<String>,
}

impl ToolResult {
    pub fn plain_text(text: impl Into<String>) -> Self {
        let text = text.into();
        Self::json_with_text(json!({ "output": text.clone() }), text)
    }

    pub fn json_with_text(json: serde_json::Value, text: impl Into<String>) -> Self {
        Self {
            output: json,
            display_text: Some(text.into()),
            warnings: Vec::new(),
        }
    }
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn execute(
        &self,
        invocation: &ToolInvocation,
        context: &ToolContext,
    ) -> Result<ToolResult, ToolError>;
}
