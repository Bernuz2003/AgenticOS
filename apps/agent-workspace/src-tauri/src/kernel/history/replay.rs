use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use crate::models::kernel::{
    WorkspaceContextSnapshot, WorkspaceReplayBaselineSnapshot, WorkspaceReplayDebugSnapshot,
    WorkspaceReplayDiffSnapshot, WorkspaceReplayInvocationDiff, WorkspaceSnapshot,
};

use super::db::{
    load_messages, load_replay_branch, load_tool_invocations, open_connection, StoredMessage,
    StoredReplayBranchRecord, StoredToolInvocation,
};

#[derive(Debug, Deserialize)]
struct ReplayBranchBaseline {
    #[serde(default)]
    context_segments: Vec<ReplayBranchSegment>,
    #[serde(default)]
    episodic_segments: Vec<ReplayBranchSegment>,
    #[serde(default)]
    replay_messages: Vec<ReplayBranchMessage>,
    #[serde(default)]
    tool_invocations: Vec<ReplayBranchInvocation>,
}

#[derive(Debug, Deserialize)]
struct ReplayBranchSegment {
    kind: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct ReplayBranchMessage {
    role: String,
    kind: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ReplayBranchInvocation {
    tool_call_id: String,
    tool_name: String,
    command_text: String,
    status: String,
    #[serde(default)]
    output_text: Option<String>,
    #[serde(default)]
    error_kind: Option<String>,
    #[serde(default)]
    kill: bool,
}

pub fn hydrate_workspace_snapshot_replay(
    workspace_root: &Path,
    snapshot: &mut WorkspaceSnapshot,
) -> Result<(), String> {
    snapshot.replay = None;

    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(());
    };
    let Some(record) = load_replay_branch(&connection, &snapshot.session_id)? else {
        return Ok(());
    };
    let baseline: ReplayBranchBaseline =
        serde_json::from_str(&record.baseline_json).map_err(|err| {
            format!(
                "failed to decode replay baseline for {}: {err}",
                snapshot.session_id
            )
        })?;
    let current_messages = load_messages(&connection, &snapshot.session_id)?;
    let current_invocations = load_tool_invocations(&connection, &snapshot.session_id)?;

    snapshot.replay = Some(build_replay_snapshot(
        record,
        baseline,
        current_messages,
        current_invocations,
        snapshot.context.as_ref(),
    ));
    Ok(())
}

fn build_replay_snapshot(
    record: StoredReplayBranchRecord,
    baseline: ReplayBranchBaseline,
    current_messages: Vec<StoredMessage>,
    current_invocations: Vec<StoredToolInvocation>,
    live_context: Option<&WorkspaceContextSnapshot>,
) -> WorkspaceReplayDebugSnapshot {
    let shared_message_prefix =
        common_message_prefix_len(&baseline.replay_messages, &current_messages);
    let replay_messages_delta =
        current_messages.len() as i64 - baseline.replay_messages.len() as i64;
    let branch_only_messages = current_messages.len().saturating_sub(shared_message_prefix);
    let latest_branch_message = current_messages
        .iter()
        .skip(shared_message_prefix)
        .rev()
        .find(|message| !message.content.trim().is_empty())
        .map(|message| message.content.clone());

    let (invocation_diffs, changed_tool_outputs, branch_only_tool_calls) =
        build_invocation_diffs(&baseline.tool_invocations, &current_invocations);

    let current_context_segments = live_context.map(|context| context.context_segments);
    let current_episodic_segments = live_context.map(|context| context.episodic_segments);

    let source_context_chars = baseline
        .context_segments
        .iter()
        .map(|segment| segment.text.len())
        .sum();
    let source_episodic_chars = baseline
        .episodic_segments
        .iter()
        .map(|segment| segment.text.len())
        .sum();
    let source_context_kinds = collect_segment_kinds(&baseline.context_segments);
    let source_episodic_kinds = collect_segment_kinds(&baseline.episodic_segments);

    WorkspaceReplayDebugSnapshot {
        source_dump_id: record.source_dump_id,
        source_session_id: record.source_session_id,
        source_pid: record.source_pid,
        source_fidelity: record.source_fidelity,
        replay_mode: record.replay_mode,
        tool_mode: record.tool_mode,
        initial_state: record.initial_state,
        patched_context_segments: record.patched_context_segments,
        patched_episodic_segments: record.patched_episodic_segments,
        stubbed_invocations: record.stubbed_invocations,
        overridden_invocations: record.overridden_invocations,
        baseline: WorkspaceReplayBaselineSnapshot {
            source_context_segments: baseline.context_segments.len(),
            source_episodic_segments: baseline.episodic_segments.len(),
            source_replay_messages: baseline.replay_messages.len(),
            source_tool_invocations: baseline.tool_invocations.len(),
            source_context_chars,
            source_episodic_chars,
            source_context_kinds,
            source_episodic_kinds,
        },
        diff: WorkspaceReplayDiffSnapshot {
            current_context_segments,
            current_episodic_segments,
            current_replay_messages: current_messages.len(),
            current_tool_invocations: current_invocations.len(),
            context_changed: current_context_segments
                .map(|count| count != baseline.context_segments.len()),
            context_segments_delta: current_context_segments
                .map(|count| count as i64 - baseline.context_segments.len() as i64),
            episodic_segments_delta: current_episodic_segments
                .map(|count| count as i64 - baseline.episodic_segments.len() as i64),
            replay_messages_delta,
            tool_invocations_delta: current_invocations.len() as i64
                - baseline.tool_invocations.len() as i64,
            branch_only_messages,
            branch_only_tool_calls,
            changed_tool_outputs,
            completed_tool_calls: current_invocations
                .iter()
                .filter(|invocation| invocation.status != "dispatched")
                .count(),
            latest_branch_message,
            invocation_diffs,
        },
    }
}

fn common_message_prefix_len(baseline: &[ReplayBranchMessage], current: &[StoredMessage]) -> usize {
    baseline
        .iter()
        .zip(current.iter())
        .take_while(|(left, right)| {
            left.role == right.role && left.kind == right.kind && left.content == right.content
        })
        .count()
}

fn build_invocation_diffs(
    baseline: &[ReplayBranchInvocation],
    current: &[StoredToolInvocation],
) -> (Vec<WorkspaceReplayInvocationDiff>, usize, usize) {
    let baseline_by_id = baseline
        .iter()
        .map(|invocation| (invocation.tool_call_id.as_str(), invocation))
        .collect::<HashMap<_, _>>();
    let baseline_by_command = baseline.iter().fold(
        HashMap::<&str, Vec<&ReplayBranchInvocation>>::new(),
        |mut acc, invocation| {
            acc.entry(invocation.command_text.as_str())
                .or_default()
                .push(invocation);
            acc
        },
    );

    let mut matched_source_ids = HashSet::<String>::new();
    let mut matched_current_ids = HashSet::<i64>::new();
    let mut matched_current_by_source = HashMap::<String, &StoredToolInvocation>::new();

    for invocation in current {
        let Some(source_id) = replay_stub_source_call_id(invocation) else {
            continue;
        };
        if baseline_by_id.contains_key(source_id.as_str()) {
            matched_source_ids.insert(source_id.clone());
            matched_current_ids.insert(invocation.invocation_id);
            matched_current_by_source.insert(source_id, invocation);
        }
    }

    for invocation in current {
        if matched_current_ids.contains(&invocation.invocation_id) {
            continue;
        }
        let Some(candidates) = baseline_by_command.get(invocation.command_text.as_str()) else {
            continue;
        };
        let Some(source) = candidates
            .iter()
            .find(|candidate| !matched_source_ids.contains(candidate.tool_call_id.as_str()))
        else {
            continue;
        };
        matched_source_ids.insert(source.tool_call_id.clone());
        matched_current_ids.insert(invocation.invocation_id);
        matched_current_by_source.insert(source.tool_call_id.clone(), invocation);
    }

    let mut invocation_diffs = Vec::new();
    let mut changed_tool_outputs = 0;

    for source in baseline {
        let replay = matched_current_by_source
            .get(source.tool_call_id.as_str())
            .copied();
        let changed = replay
            .map(|current| tool_invocation_changed(source, current))
            .unwrap_or(true);
        if replay.is_some_and(|current| tool_invocation_changed(source, current)) {
            changed_tool_outputs += 1;
        }
        if changed {
            invocation_diffs.push(WorkspaceReplayInvocationDiff {
                source_tool_call_id: Some(source.tool_call_id.clone()),
                replay_tool_call_id: replay.map(|current| current.tool_call_id.clone()),
                tool_name: source.tool_name.clone(),
                command_text: source.command_text.clone(),
                source_status: Some(source.status.clone()),
                replay_status: replay.map(|current| current.status.clone()),
                source_output_text: source.output_text.clone(),
                replay_output_text: replay.and_then(|current| current.output_text.clone()),
                branch_only: false,
                changed: true,
            });
        }
    }

    let mut branch_only_tool_calls = 0;
    for invocation in current {
        if matched_current_ids.contains(&invocation.invocation_id) {
            continue;
        }
        branch_only_tool_calls += 1;
        invocation_diffs.push(WorkspaceReplayInvocationDiff {
            source_tool_call_id: None,
            replay_tool_call_id: Some(invocation.tool_call_id.clone()),
            tool_name: invocation.tool_name.clone(),
            command_text: invocation.command_text.clone(),
            source_status: None,
            replay_status: Some(invocation.status.clone()),
            source_output_text: None,
            replay_output_text: invocation.output_text.clone(),
            branch_only: true,
            changed: true,
        });
    }

    invocation_diffs.truncate(8);
    (
        invocation_diffs,
        changed_tool_outputs,
        branch_only_tool_calls,
    )
}

fn tool_invocation_changed(source: &ReplayBranchInvocation, replay: &StoredToolInvocation) -> bool {
    source.status != replay.status
        || source.output_text != replay.output_text
        || source.error_kind != replay.error_kind
        || source.kill != replay.kill
}

fn replay_stub_source_call_id(invocation: &StoredToolInvocation) -> Option<String> {
    let warnings_json = invocation.warnings_json.as_deref()?;
    let warnings = serde_json::from_str::<Vec<String>>(warnings_json).ok()?;
    warnings.into_iter().find_map(|warning| {
        warning
            .strip_prefix("replay_stub_source_call_id=")
            .map(ToString::to_string)
    })
}

fn collect_segment_kinds(segments: &[ReplayBranchSegment]) -> Vec<String> {
    let mut kinds = Vec::new();
    for segment in segments {
        if kinds.iter().any(|existing| existing == &segment.kind) {
            continue;
        }
        kinds.push(segment.kind.clone());
    }
    kinds
}
