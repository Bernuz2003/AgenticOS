use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActionName {
    Spawn,
    Send,
    Receive,
    Ack,
}

impl ActionName {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Spawn => "spawn",
            Self::Send => "send",
            Self::Receive => "receive",
            Self::Ack => "ack",
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
        "receive" => ActionName::Receive,
        "ack" => ActionName::Ack,
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
            description: "Queue a typed IPC message for a PID, workflow task, role or channel."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pid": {"type": "integer", "minimum": 0},
                    "task": {"type": "string"},
                    "role": {"type": "string"},
                    "orchestration_id": {"type": "integer", "minimum": 0},
                    "message": {"type": "string"},
                    "message_type": {
                        "type": "string",
                        "enum": ["request", "response", "event", "notification", "handoff", "control"]
                    },
                    "channel": {"type": "string"},
                    "payload": {}
                },
                "additionalProperties": false
            }),
            input_example: json!({
                "pid": 0,
                "message_type": "notification",
                "message": "string"
            }),
            notes: vec![
                "Use pid targeting for legacy/direct compatibility; prefer task/role/channel inside workflows."
                    .to_string(),
                "The legacy 'message' field remains supported for compatibility.".to_string(),
            ],
        },
        ActionDescriptor {
            name: ActionName::Receive.as_str().to_string(),
            description: "Read pending IPC messages from the current process mailbox."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "minimum": 1, "maximum": 16},
                    "channel": {"type": "string"},
                    "include_delivered": {"type": "boolean"}
                },
                "additionalProperties": false
            }),
            input_example: json!({
                "limit": 4,
                "channel": "review"
            }),
            notes: vec![
                "Queued messages become delivered when received.".to_string(),
                "Use ACTION:ack after processing messages to mark them consumed."
                    .to_string(),
            ],
        },
        ActionDescriptor {
            name: ActionName::Ack.as_str().to_string(),
            description: "Mark previously received IPC messages as consumed.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["message_ids"],
                "properties": {
                    "message_ids": {
                        "type": "array",
                        "items": {"type": "string"},
                        "minItems": 1
                    }
                },
                "additionalProperties": false
            }),
            input_example: json!({
                "message_ids": ["ipc-123"]
            }),
            notes: vec![
                "Ack only messages you have actually processed.".to_string(),
            ],
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
    fn parses_receive_action() {
        let parsed = parse_text_invocation(r#"ACTION:receive {"limit":2}"#).expect("action parse");
        assert_eq!(parsed.action, ActionName::Receive);
        assert_eq!(parsed.input["limit"], 2);
    }

    #[test]
    fn rejects_unknown_action() {
        let err =
            parse_text_invocation(r#"ACTION:fork {"prompt":"hello"}"#).expect_err("unknown action");
        assert!(err.to_string().contains("Unknown action"));
    }
}
