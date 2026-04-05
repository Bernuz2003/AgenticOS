use crate::scheduler::ProcessScheduler;
use crate::tools::parser::parse_text_invocation;
use crate::tools::SysCallOutcome;

use super::worker::SyscallCompletion;
use crate::tools::invocation::ToolCaller;

pub(super) fn replay_stubbed_completion(
    scheduler: &mut ProcessScheduler,
    pid: u64,
    tool_call_id: &str,
    command: &str,
    caller: &ToolCaller,
) -> Option<SyscallCompletion> {
    let metadata = scheduler.replay_process(pid)?;
    if !metadata.tool_mode.is_stubbed() {
        return None;
    }

    let outcome = if let Some(stub) = scheduler.consume_replay_tool_stub(pid, command) {
        let mut outcome = stub.outcome;
        outcome
            .warnings
            .push(format!("replay_stub_source_call_id={}", stub.tool_call_id));
        outcome
            .warnings
            .push(format!("replay_stub_source_tool={}", stub.tool_name));
        outcome
    } else {
        missing_replay_stub_outcome(command)
    };

    Some(SyscallCompletion {
        pid,
        tool_call_id: tool_call_id.to_string(),
        command: command.to_string(),
        caller: caller.clone(),
        outcome,
    })
}

fn missing_replay_stub_outcome(command: &str) -> SysCallOutcome {
    let tool_name = parse_text_invocation(command)
        .map(|invocation| invocation.name)
        .unwrap_or_else(|_| "unknown".to_string());
    SysCallOutcome {
        output: format!(
            "Replay Stub Error: no recorded invocation matched '{}'. Live tool execution is disabled for this replay branch.",
            tool_name
        ),
        success: false,
        duration_ms: 0,
        should_kill_process: false,
        output_json: None,
        warnings: vec![format!("replay_stub_missing_command={}", command.trim())],
        error_kind: Some("replay_stub_missing".to_string()),
        effects: Vec::new(),
    }
}
