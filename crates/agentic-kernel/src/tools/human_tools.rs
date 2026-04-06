use agentic_kernel_macros::agentic_tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::process::{HumanInputRequest, HumanInputRequestKind};
use crate::storage::current_timestamp_ms;

use super::error::ToolError;
use super::invocation::ToolContext;

static HUMAN_REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_human_request_id() -> String {
    let seq = HUMAN_REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("human-{}-{seq}", current_timestamp_ms())
}

pub(crate) fn build_approval_request(
    question: impl Into<String>,
    details: Option<String>,
) -> HumanInputRequest {
    HumanInputRequest {
        request_id: next_human_request_id(),
        kind: HumanInputRequestKind::Approval,
        question: question.into(),
        details: details
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        choices: vec!["approve".to_string(), "reject".to_string()],
        allow_free_text: false,
        placeholder: None,
        requested_at_ms: current_timestamp_ms(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AskHumanKind {
    #[default]
    Question,
    Approval,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AskHumanInput {
    question: String,
    #[serde(default)]
    details: Option<String>,
    #[serde(default)]
    choices: Vec<String>,
    #[serde(default)]
    allow_free_text: Option<bool>,
    #[serde(default)]
    placeholder: Option<String>,
    #[serde(default)]
    kind: AskHumanKind,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct AskHumanOutput {
    output: String,
    status: String,
    kind: String,
    question: String,
    choices: Vec<String>,
    allow_free_text: bool,
}

pub(crate) fn normalize_ask_human_request(
    input: AskHumanInput,
) -> Result<HumanInputRequest, ToolError> {
    let question = input.question.trim();
    if question.is_empty() {
        return Err(ToolError::InvalidInput(
            "ask_human".into(),
            "field 'question' cannot be empty".into(),
        ));
    }

    let mut choices = input
        .choices
        .into_iter()
        .map(|choice| choice.trim().to_string())
        .filter(|choice| !choice.is_empty())
        .collect::<Vec<_>>();
    choices.dedup();

    let kind = match input.kind {
        AskHumanKind::Question => HumanInputRequestKind::Question,
        AskHumanKind::Approval => HumanInputRequestKind::Approval,
    };
    if matches!(kind, HumanInputRequestKind::Approval) && choices.is_empty() {
        choices = vec!["approve".to_string(), "reject".to_string()];
    }

    let allow_free_text = input
        .allow_free_text
        .unwrap_or(!matches!(kind, HumanInputRequestKind::Approval));
    let details = input
        .details
        .map(|details| details.trim().to_string())
        .filter(|details| !details.is_empty());
    let placeholder = input
        .placeholder
        .map(|placeholder| placeholder.trim().to_string())
        .filter(|placeholder| !placeholder.is_empty());

    Ok(HumanInputRequest {
        request_id: next_human_request_id(),
        kind,
        question: question.to_string(),
        details,
        choices,
        allow_free_text,
        placeholder,
        requested_at_ms: current_timestamp_ms(),
    })
}

#[agentic_tool(
    name = "ask_human",
    description = "Pause the current process and request structured input or approval from a human operator.",
    input_example = serde_json::json!({"question": "Ship this patch?", "kind": "approval"}),
    capabilities = ["hitl", "approval"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn ask_human(input: AskHumanInput, _ctx: &ToolContext) -> Result<AskHumanOutput, ToolError> {
    let request = normalize_ask_human_request(input)?;
    let output = match request.kind {
        HumanInputRequestKind::Question => {
            format!("Human input requested: {}", request.question)
        }
        HumanInputRequestKind::Approval => {
            format!("Human approval requested: {}", request.question)
        }
    };

    Ok(AskHumanOutput {
        output,
        status: "pending_human_input".to_string(),
        kind: request.kind.as_str().to_string(),
        question: request.question,
        choices: request.choices,
        allow_free_text: request.allow_free_text,
    })
}

#[cfg(test)]
mod tests {
    use super::{normalize_ask_human_request, AskHumanInput, AskHumanKind};
    use crate::process::HumanInputRequestKind;

    #[test]
    fn approval_defaults_to_approve_reject_and_blocks_free_text() {
        let request = normalize_ask_human_request(AskHumanInput {
            question: "Ship this patch?".to_string(),
            details: None,
            choices: Vec::new(),
            allow_free_text: None,
            placeholder: None,
            kind: AskHumanKind::Approval,
        })
        .expect("approval request");

        assert_eq!(request.kind, HumanInputRequestKind::Approval);
        assert_eq!(request.choices, vec!["approve", "reject"]);
        assert!(!request.allow_free_text);
    }

    #[test]
    fn question_request_trims_details_and_keeps_free_text_enabled() {
        let request = normalize_ask_human_request(AskHumanInput {
            question: "  Which repository should I inspect next? ".to_string(),
            details: Some("  kernel vs gui  ".to_string()),
            choices: vec![" kernel ".to_string(), "gui".to_string(), "gui".to_string()],
            allow_free_text: None,
            placeholder: Some("  Write your answer  ".to_string()),
            kind: AskHumanKind::Question,
        })
        .expect("question request");

        assert_eq!(request.kind, HumanInputRequestKind::Question);
        assert_eq!(request.question, "Which repository should I inspect next?");
        assert_eq!(request.details.as_deref(), Some("kernel vs gui"));
        assert_eq!(request.choices, vec!["kernel", "gui"]);
        assert!(request.allow_free_text);
        assert_eq!(request.placeholder.as_deref(), Some("Write your answer"));
    }
}
