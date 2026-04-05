use serde_json::{json, Value};

use crate::config::kernel_config;
use crate::session::SessionRegistry;
use crate::storage::{CompletedToolInvocationRecord, NewToolInvocationRecord, StorageService};
use crate::tools::invocation::{ToolCaller, ToolInvocationTransport};
use crate::tools::SysCallOutcome;

#[derive(Debug, Clone, Default)]
pub(crate) struct ToolInvocationCompletionData {
    pub(crate) output_json: Option<Value>,
    pub(crate) output_text: Option<String>,
    pub(crate) warnings: Vec<String>,
    pub(crate) error_kind: Option<String>,
    pub(crate) error_text: Option<String>,
    pub(crate) effects: Vec<Value>,
    pub(crate) duration_ms: Option<u128>,
    pub(crate) kill: bool,
}

pub(crate) fn record_tool_invocation_dispatched(
    storage: &mut StorageService,
    session_registry: &SessionRegistry,
    runtime_id: &str,
    pid: u64,
    tool_call_id: &str,
    command: &str,
    caller: &ToolCaller,
) -> Result<(), String> {
    let (tool_name, input_json) = parse_tool_command(command)?;
    storage
        .record_tool_invocation_dispatch(
            &NewToolInvocationRecord {
                tool_call_id: tool_call_id.to_string(),
                session_id: session_registry
                    .session_id_for_pid(pid)
                    .map(ToString::to_string),
                pid: Some(pid),
                runtime_id: Some(runtime_id.to_string()),
                tool_name,
                caller: caller.as_str().to_string(),
                transport: ToolInvocationTransport::Text.as_str().to_string(),
                status: "dispatched".to_string(),
                command_text: command.to_string(),
                input_json,
            },
            kernel_config().core_dump.max_tool_invocations_per_pid,
        )
        .map_err(|err| err.to_string())
}

pub(crate) fn complete_tool_invocation_from_outcome(
    storage: &mut StorageService,
    tool_call_id: &str,
    status: &str,
    outcome: &SysCallOutcome,
) -> Result<(), String> {
    complete_tool_invocation(
        storage,
        tool_call_id,
        status,
        ToolInvocationCompletionData {
            output_json: outcome.output_json.clone(),
            output_text: Some(outcome.output.clone()),
            warnings: outcome.warnings.clone(),
            error_kind: outcome.error_kind.clone(),
            error_text: (!outcome.success).then_some(outcome.output.clone()),
            effects: outcome.effects.clone(),
            duration_ms: Some(outcome.duration_ms),
            kill: outcome.should_kill_process,
        },
    )
}

pub(crate) fn complete_tool_invocation(
    storage: &mut StorageService,
    tool_call_id: &str,
    status: &str,
    completion: ToolInvocationCompletionData,
) -> Result<(), String> {
    let warnings_json = if completion.warnings.is_empty() {
        None
    } else {
        Some(
            serde_json::to_string(&completion.warnings)
                .map_err(|err| format!("serialize invocation warnings: {err}"))?,
        )
    };
    let effect_json = if completion.effects.is_empty() {
        None
    } else {
        Some(
            serde_json::to_string(&completion.effects)
                .map_err(|err| format!("serialize invocation effects: {err}"))?,
        )
    };
    let output_json = completion
        .output_json
        .map(|value| serde_json::to_string(&value))
        .transpose()
        .map_err(|err| format!("serialize invocation output: {err}"))?;

    storage
        .complete_tool_invocation(&CompletedToolInvocationRecord {
            tool_call_id: tool_call_id.to_string(),
            status: status.to_string(),
            output_json,
            output_text: completion.output_text,
            warnings_json,
            error_kind: completion.error_kind,
            error_text: completion.error_text,
            effect_json,
            duration_ms: completion.duration_ms,
            kill: completion.kill,
        })
        .map_err(|err| err.to_string())
}

fn parse_tool_command(command: &str) -> Result<(String, String), String> {
    match crate::tools::parser::parse_text_invocation(command) {
        Ok(invocation) => {
            let input_json = serde_json::to_string(&invocation.input)
                .map_err(|err| format!("serialize tool input: {err}"))?;
            Ok((invocation.name, input_json))
        }
        Err(_) => {
            let tool_name = command
                .trim()
                .strip_prefix("TOOL:")
                .and_then(|rest| {
                    let end = rest
                        .find(|ch: char| ch.is_whitespace() || ch == '{')
                        .unwrap_or(rest.len());
                    let candidate = rest[..end].trim();
                    (!candidate.is_empty()).then_some(candidate.to_string())
                })
                .unwrap_or_else(|| "unknown".to_string());
            Ok((
                tool_name,
                json!({
                    "_raw_command": command,
                })
                .to_string(),
            ))
        }
    }
}
