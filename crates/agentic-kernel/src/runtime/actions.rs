use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActionName {
    Spawn,
    Send,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ActionInvocation {
    pub(crate) action: ActionName,
    pub(crate) input: Value,
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

pub(crate) fn is_streaming_action_invocation(text: &str) -> bool {
    crate::text_invocation::is_streaming_prefixed_json_invocation(text, "ACTION:")
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
