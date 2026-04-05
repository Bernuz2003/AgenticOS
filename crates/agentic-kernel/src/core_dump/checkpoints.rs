use serde_json::Value;

use super::models::{
    CoreDumpAssistantSegment, CoreDumpDebugCheckpoint, CoreDumpTurnAssembly,
    DebugCheckpointInvocation, DebugCheckpointSnapshot,
};
use crate::config::kernel_config;
use crate::process::{AgentProcess, ProcessState};
use crate::runtime::{TurnAssemblySnapshot, TurnAssemblyStore};
use crate::session::SessionRegistry;
use crate::storage::{NewDebugCheckpointRecord, StorageService};

pub(crate) fn record_live_debug_checkpoint(
    storage: &mut StorageService,
    session_registry: &SessionRegistry,
    turn_assembly: &TurnAssemblyStore,
    runtime_id: &str,
    pid: u64,
    process: &AgentProcess,
    boundary: &str,
    invocation: Option<DebugCheckpointInvocation>,
) -> Result<(), String> {
    let rendered_prompt = turn_assembly.render_inference_prompt(
        pid,
        process.prompt_text(),
        process.resident_prompt_checkpoint_bytes(),
    );
    let snapshot = DebugCheckpointSnapshot {
        prompt_text: process.prompt_text().to_string(),
        resident_prompt_checkpoint_bytes: process.resident_prompt_checkpoint_bytes(),
        rendered_inference_prompt: rendered_prompt.full_prompt,
        context_policy: process.context_policy.clone(),
        context_state: process.context_state.clone(),
        pending_human_request: process.pending_human_request.clone(),
        termination_reason: process.termination_reason.clone(),
        turn_assembly: turn_assembly.snapshot(pid).map(map_turn_assembly_snapshot),
        invocation,
    };
    let snapshot_json =
        serde_json::to_string(&snapshot).map_err(|err| format!("serialize checkpoint: {err}"))?;
    storage
        .record_debug_checkpoint(
            &NewDebugCheckpointRecord {
                session_id: session_registry
                    .session_id_for_pid(pid)
                    .map(ToString::to_string),
                pid: Some(pid),
                runtime_id: Some(runtime_id.to_string()),
                boundary: boundary.to_string(),
                state: process_state_label(&process.state).to_string(),
                snapshot_json,
            },
            kernel_config().core_dump.max_debug_checkpoints_per_pid,
        )
        .map_err(|err| err.to_string())?;
    Ok(())
}

pub(crate) fn load_manifest_debug_checkpoints(
    storage: &StorageService,
    pid: u64,
) -> Result<Vec<CoreDumpDebugCheckpoint>, String> {
    storage
        .recent_debug_checkpoints_for_pid(
            pid,
            kernel_config().core_dump.max_debug_checkpoints_per_pid,
        )
        .map_err(|err| err.to_string())?
        .into_iter()
        .rev()
        .map(|record| {
            let snapshot =
                serde_json::from_str::<DebugCheckpointSnapshot>(&record.snapshot_json)
                    .map_err(|err| format!("parse checkpoint {}: {err}", record.checkpoint_id))?;
            Ok(CoreDumpDebugCheckpoint {
                checkpoint_id: record.checkpoint_id,
                recorded_at_ms: record.recorded_at_ms,
                boundary: record.boundary,
                state: record.state,
                snapshot,
            })
        })
        .collect()
}

pub(crate) fn map_turn_assembly_snapshot(snapshot: TurnAssemblySnapshot) -> CoreDumpTurnAssembly {
    CoreDumpTurnAssembly {
        raw_transport_text: snapshot.raw_transport_text,
        visible_projection: snapshot.visible_projection,
        thinking_projection: snapshot.thinking_projection,
        pending_invocation: snapshot.pending_invocation,
        pending_segments: snapshot
            .pending_segments
            .into_iter()
            .map(|segment| CoreDumpAssistantSegment {
                kind: segment.kind,
                text: segment.text,
            })
            .collect(),
        output_stop_requested: snapshot.output_stop_requested,
        generated_token_count: snapshot.generated_token_count,
    }
}

pub(crate) fn process_state_label(state: &ProcessState) -> &'static str {
    match state {
        ProcessState::Ready => "ready",
        ProcessState::Running => "running",
        ProcessState::AwaitingTurnDecision => "awaiting_turn_decision",
        ProcessState::WaitingForInput => "waiting_for_input",
        ProcessState::WaitingForHumanInput => "waiting_for_human_input",
        ProcessState::Parked => "parked",
        ProcessState::WaitingForSyscall => "waiting_for_syscall",
        ProcessState::Finished => "finished",
    }
}

pub(crate) fn invocation_marker(
    tool_call_id: Option<&str>,
    command: Option<&str>,
    status: Option<&str>,
) -> Option<DebugCheckpointInvocation> {
    if tool_call_id.is_none() && command.is_none() && status.is_none() {
        None
    } else {
        Some(DebugCheckpointInvocation {
            tool_call_id: tool_call_id.map(ToString::to_string),
            command: command.map(ToString::to_string),
            status: status.map(ToString::to_string),
        })
    }
}

pub(crate) fn parse_json_array_strings(raw: Option<&str>) -> Result<Vec<String>, String> {
    match raw {
        Some(raw) => serde_json::from_str::<Vec<String>>(raw)
            .map_err(|err| format!("parse json string array: {err}")),
        None => Ok(Vec::new()),
    }
}

pub(crate) fn parse_json_array_values(raw: Option<&str>) -> Result<Vec<Value>, String> {
    match raw {
        Some(raw) => serde_json::from_str::<Vec<Value>>(raw)
            .map_err(|err| format!("parse json value array: {err}")),
        None => Ok(Vec::new()),
    }
}
