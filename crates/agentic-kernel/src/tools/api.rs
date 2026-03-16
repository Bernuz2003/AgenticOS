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

pub(crate) fn typed_output_to_tool_result<T: Serialize>(
    tool_name: &str,
    output: T,
) -> Result<ToolResult, ToolError> {
    let output = serde_json::to_value(output).map_err(|err| {
        ToolError::Internal(format!(
            "Failed to serialize output for tool '{}': {}",
            tool_name, err
        ))
    })?;

    let display_text = output
        .as_object()
        .and_then(|object| object.get("output"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| serde_json::to_string_pretty(&output).ok());

    Ok(ToolResult {
        output,
        display_text,
        warnings: Vec::new(),
    })
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn execute(
        &self,
        invocation: &ToolInvocation,
        context: &ToolContext,
    ) -> Result<ToolResult, ToolError>;
}
