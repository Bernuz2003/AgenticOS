use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActionName {
    Spawn,
    Send,
}

impl ActionName {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Spawn => "spawn",
            Self::Send => "send",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ActionInvocation {
    pub(crate) action: ActionName,
    pub(crate) input: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ActionDescriptor {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema: Value,
    pub(crate) input_example: Value,
    pub(crate) notes: Vec<String>,
}

#[derive(Debug, Clone, Error, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum ActionError {
    #[error("Malformed action invocation: {0}")]
    MalformedInvocation(String),

    #[error("Unknown action '{0}'")]
    UnknownAction(String),
}

pub(crate) fn parse_text_invocation(text: &str) -> Result<ActionInvocation, ActionError> {
    let (name, input) = crate::text_invocation::parse_prefixed_json_invocation(text, "ACTION:")
        .map_err(ActionError::MalformedInvocation)?;

    let action = match name.trim().to_ascii_lowercase().as_str() {
        "spawn" => ActionName::Spawn,
        "send" => ActionName::Send,
        other => return Err(ActionError::UnknownAction(other.to_string())),
    };

    Ok(ActionInvocation { action, input })
}

#[allow(dead_code)]
pub(crate) fn is_streaming_action_invocation(text: &str) -> bool {
    crate::text_invocation::is_streaming_prefixed_json_invocation(text, "ACTION:")
}

pub(crate) fn builtin_action_descriptors() -> Vec<ActionDescriptor> {
    vec![
        ActionDescriptor {
            name: ActionName::Spawn.as_str().to_string(),
            description: "Create a child LLM process under the current runtime binding."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["prompt"],
                "properties": {
                    "prompt": {"type": "string"}
                },
                "additionalProperties": false
            }),
            input_example: json!({"prompt": "string"}),
            notes: vec!["Actions mutate the runtime/process graph and are not tools.".to_string()],
        },
        ActionDescriptor {
            name: ActionName::Send.as_str().to_string(),
            description: "Send a message to another running PID.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["pid", "message"],
                "properties": {
                    "pid": {"type": "integer", "minimum": 0},
                    "message": {"type": "string"}
                },
                "additionalProperties": false
            }),
            input_example: json!({"pid": 0, "message": "string"}),
            notes: vec!["Use this only when you already know the target PID.".to_string()],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{parse_text_invocation, ActionName};

    #[test]
    fn parses_spawn_action() {
        let parsed =
            parse_text_invocation(r#"ACTION:spawn {"prompt":"hello"}"#).expect("action parse");
        assert_eq!(parsed.action, ActionName::Spawn);
        assert_eq!(parsed.input["prompt"], "hello");
    }

    #[test]
    fn rejects_unknown_action() {
        let err =
            parse_text_invocation(r#"ACTION:fork {"prompt":"hello"}"#).expect_err("unknown action");
        assert!(err.to_string().contains("Unknown action"));
    }
}
