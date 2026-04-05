use std::collections::{HashMap, VecDeque};
use std::path::Path;

use agentic_control_models::{
    CoreDumpReplayPatch, CoreDumpReplayRequest, CoreDumpReplayResult, CoreDumpReplaySegmentPatch,
    CoreDumpReplayToolOutputOverride, KernelEvent,
};
use agentic_protocol::ControlErrorCode;
use serde_json::Value;

use super::models::{AgentCoreDumpManifest, CoreDumpProcessMetadata, CoreDumpToolInvocation};
use super::package::load_manifest;
use super::{build_replay_branch_baseline, ReplayBranchBaseline};
use crate::commands::{runtime_selector_for_session, ProcessCommandContext};
use crate::diagnostics::audit::{self, AuditContext};
use crate::model_catalog::parse_workload_label;
use crate::process::{ContextSegment, ContextSegmentKind, ProcessLifecyclePolicy, ProcessState};
use crate::scheduler::{
    ProcessPriority, ReplayPatchSummary, ReplayProcessMetadata, ReplayToolMode, ReplayToolStub,
};
use crate::services::model_runtime::activate_model_target;
use crate::services::process_runtime::{
    kill_managed_process_with_session, spawn_restored_managed_process_with_session,
    RestoredManagedProcessRequest,
};
use crate::storage::NewReplayBranchRecord;
use crate::storage::StoredReplayMessage;
use crate::tools::invocation::ToolCaller;
use crate::tools::SysCallOutcome;

pub(crate) fn replay_core_dump(
    ctx: &mut ProcessCommandContext<'_>,
    request: CoreDumpReplayRequest,
) -> Result<CoreDumpReplayResult, (ControlErrorCode, String)> {
    let dump_id = request.dump_id.trim().to_string();
    if dump_id.is_empty() {
        return Err((
            ControlErrorCode::CoreDumpReplayInvalid,
            "REPLAY_COREDUMP requires a non-empty dump_id".to_string(),
        ));
    }

    let Some(record) = ctx
        .storage
        .core_dump_record(&dump_id)
        .map_err(storage_err(ControlErrorCode::CoreDumpReplayFailed))?
    else {
        return Err((
            ControlErrorCode::CoreDumpNotFound,
            format!("core dump '{}' not found", dump_id),
        ));
    };
    let manifest = load_manifest(Path::new(&record.path))
        .map_err(protocol_err(ControlErrorCode::CoreDumpReplayFailed))?;

    let process = manifest.process.as_ref().ok_or_else(|| {
        (
            ControlErrorCode::CoreDumpReplayFailed,
            format!(
                "core dump '{}' does not include process metadata required for replay",
                dump_id
            ),
        )
    })?;
    if process.rendered_inference_prompt.trim().is_empty() {
        return Err((
            ControlErrorCode::CoreDumpReplayFailed,
            format!(
                "core dump '{}' does not include a rendered inference prompt for replay",
                dump_id
            ),
        ));
    }

    let patch = request.patch.unwrap_or_default();
    let tool_mode = resolve_replay_tool_mode(request.tool_mode.as_deref())?;
    let (context_state, patch_summary) = build_replay_context_state(process, &patch)
        .map_err(protocol_err(ControlErrorCode::CoreDumpReplayInvalid))?;
    let stubbed_invocations = build_replay_tool_stubs(&manifest, &patch, tool_mode)
        .map_err(protocol_err(ControlErrorCode::CoreDumpReplayInvalid))?;
    let stubbed_invocation_count = stubbed_invocations.len();
    let initial_state = normalize_replay_process_state(
        manifest.target.state.as_str(),
        process.pending_human_request.is_some(),
    );
    let replay_mode = replay_mode_label(tool_mode, &patch_summary);
    let replay_baseline = build_replay_branch_baseline(&manifest, process);

    let runtime_id = resolve_replay_runtime_id(ctx, &manifest)
        .and_then(|runtime_id| ensure_replay_runtime_loaded(ctx, &runtime_id))?;
    let session_prompt = replay_session_prompt(&manifest, request.branch_label.as_deref());
    let session_id = ctx
        .session_registry
        .open_session(ctx.storage, &session_prompt, &runtime_id)
        .map_err(|err| (ControlErrorCode::CoreDumpReplayFailed, err.to_string()))?;
    let workload_label = replay_workload_label(ctx, &manifest);
    let workload = parse_workload_label(&workload_label).unwrap_or_default();
    let permission_policy = process
        .permission_policy
        .derive_replay_safe(ctx.tool_registry);
    let pid_floor = ctx.runtime_registry.next_pid_floor();
    let tool_caller = parse_tool_caller(&process.tool_caller);

    let spawned = {
        let Some(engine) = ctx.runtime_registry.engine_mut(&runtime_id) else {
            return Err((
                ControlErrorCode::NoModel,
                format!(
                    "runtime '{}' is not available for core dump replay",
                    runtime_id
                ),
            ));
        };

        spawn_restored_managed_process_with_session(
            &runtime_id,
            &session_id,
            pid_floor,
            engine,
            ctx.memory,
            ctx.scheduler,
            ctx.session_registry,
            ctx.storage,
            RestoredManagedProcessRequest {
                rendered_prompt: process.rendered_inference_prompt.clone(),
                owner_id: ctx.client_id,
                tool_caller,
                permission_policy: Some(permission_policy),
                workload,
                required_backend_class: None,
                priority: ProcessPriority::Normal,
                lifecycle_policy: ProcessLifecyclePolicy::Interactive,
                context_policy: Some(process.context_policy.clone()),
            },
        )
        .map_err(|err| {
            let _ = ctx
                .session_registry
                .delete_session(ctx.storage, &session_id);
            (
                ControlErrorCode::CoreDumpReplayFailed,
                format!(
                    "failed to spawn replay branch from dump '{}': {}",
                    dump_id, err
                ),
            )
        })?
    };
    if let Err(err) = ctx
        .runtime_registry
        .register_pid(ctx.storage, &runtime_id, spawned.pid)
    {
        cleanup_failed_replay(ctx, &runtime_id, &session_id, spawned.pid);
        return Err((
            ControlErrorCode::CoreDumpReplayFailed,
            format!(
                "failed to register replay pid '{}' on runtime '{}': {}",
                spawned.pid, runtime_id, err
            ),
        ));
    }

    if let Err(err) = apply_replay_process_snapshot(
        ctx,
        &runtime_id,
        spawned.pid,
        process,
        context_state,
        initial_state.clone(),
        tool_mode,
        stubbed_invocations.clone(),
    ) {
        cleanup_failed_replay(ctx, &runtime_id, &session_id, spawned.pid);
        return Err((ControlErrorCode::CoreDumpReplayFailed, err));
    }

    if let Err(err) = seed_replay_history(ctx, &session_id, spawned.pid, &workload_label, &manifest)
    {
        cleanup_failed_replay(ctx, &runtime_id, &session_id, spawned.pid);
        return Err((ControlErrorCode::CoreDumpReplayFailed, err));
    }
    if let Err(err) = record_replay_branch_metadata(
        ctx,
        &session_id,
        spawned.pid,
        &dump_id,
        &manifest,
        &replay_mode,
        tool_mode,
        &initial_state,
        &patch_summary,
        stubbed_invocation_count,
        &replay_baseline,
    ) {
        cleanup_failed_replay(ctx, &runtime_id, &session_id, spawned.pid);
        return Err((ControlErrorCode::CoreDumpReplayFailed, err));
    }

    let replay_title = ctx
        .session_registry
        .session(&session_id)
        .map(|record| record.title.clone())
        .unwrap_or_else(|| session_prompt.clone());
    audit::record(
        ctx.storage,
        audit::PROCESS_REPLAY_STARTED,
        format!(
            "dump_id={} source_session={} replay_mode={} tool_mode={} patched_context_segments={} patched_episodic_segments={} overridden_invocations={}",
            dump_id,
            manifest.target.session_id.as_deref().unwrap_or("unknown"),
            replay_mode,
            tool_mode.as_str(),
            patch_summary.patched_context_segments,
            patch_summary.patched_episodic_segments,
            patch_summary.overridden_invocations
        ),
        AuditContext::for_process(Some(&session_id), spawned.pid, Some(&runtime_id)),
    );
    ctx.pending_events.push(KernelEvent::SessionStarted {
        session_id: session_id.clone(),
        pid: spawned.pid,
        workload: workload_label.clone(),
        prompt: first_user_prompt(&manifest).unwrap_or_else(|| replay_title.clone()),
    });
    ctx.pending_events.push(KernelEvent::WorkspaceChanged {
        pid: spawned.pid,
        reason: "core_dump_replay_started".to_string(),
    });
    ctx.pending_events.push(KernelEvent::LobbyChanged {
        reason: "core_dump_replay_started".to_string(),
    });

    Ok(CoreDumpReplayResult {
        source_dump_id: dump_id,
        session_id,
        pid: spawned.pid,
        runtime_id,
        replay_session_title: replay_title,
        replay_fidelity: manifest.capture.fidelity,
        replay_mode,
        tool_mode: tool_mode.as_str().to_string(),
        initial_state: process_state_label(&initial_state).to_string(),
        patched_context_segments: patch_summary.patched_context_segments,
        patched_episodic_segments: patch_summary.patched_episodic_segments,
        stubbed_invocations: if tool_mode.is_stubbed() {
            stubbed_invocation_count
        } else {
            0
        },
        overridden_invocations: patch_summary.overridden_invocations,
    })
}

#[allow(clippy::too_many_arguments)]
fn record_replay_branch_metadata(
    ctx: &mut ProcessCommandContext<'_>,
    session_id: &str,
    pid: u64,
    dump_id: &str,
    manifest: &AgentCoreDumpManifest,
    replay_mode: &str,
    tool_mode: ReplayToolMode,
    initial_state: &ProcessState,
    patch_summary: &ReplayPatchSummary,
    stubbed_invocation_count: usize,
    baseline: &ReplayBranchBaseline,
) -> Result<(), String> {
    let baseline_json = serde_json::to_string(baseline)
        .map_err(|err| format!("failed to encode replay baseline: {err}"))?;
    ctx.storage
        .record_replay_branch(&NewReplayBranchRecord {
            session_id: session_id.to_string(),
            pid,
            source_dump_id: dump_id.to_string(),
            source_session_id: manifest.target.session_id.clone(),
            source_pid: manifest.target.pid,
            source_fidelity: manifest.capture.fidelity.clone(),
            replay_mode: replay_mode.to_string(),
            tool_mode: tool_mode.as_str().to_string(),
            initial_state: process_state_label(initial_state).to_string(),
            patched_context_segments: patch_summary.patched_context_segments,
            patched_episodic_segments: patch_summary.patched_episodic_segments,
            stubbed_invocations: if tool_mode.is_stubbed() {
                stubbed_invocation_count
            } else {
                0
            },
            overridden_invocations: patch_summary.overridden_invocations,
            baseline_json,
        })
        .map_err(|err| format!("failed to persist replay branch metadata: {err}"))
}

fn resolve_replay_runtime_id(
    ctx: &ProcessCommandContext<'_>,
    manifest: &AgentCoreDumpManifest,
) -> Result<String, (ControlErrorCode, String)> {
    manifest
        .runtime
        .as_ref()
        .map(|runtime| runtime.runtime_id.clone())
        .or_else(|| manifest.target.runtime_id.clone())
        .or_else(|| {
            manifest
                .target
                .session_id
                .as_deref()
                .and_then(|session_id| ctx.session_registry.runtime_id_for_session(session_id))
                .map(ToString::to_string)
        })
        .ok_or_else(|| {
            (
                ControlErrorCode::CoreDumpReplayFailed,
                "core dump replay could not resolve a runtime binding".to_string(),
            )
        })
}

fn ensure_replay_runtime_loaded(
    ctx: &mut ProcessCommandContext<'_>,
    runtime_id: &str,
) -> Result<String, (ControlErrorCode, String)> {
    if ctx.runtime_registry.is_runtime_loaded(runtime_id) {
        return Ok(runtime_id.to_string());
    }

    let selector = runtime_selector_for_session(ctx.runtime_registry, runtime_id)
        .map_err(protocol_err(ControlErrorCode::NoModel))?;
    if let Err(err) = ctx.model_catalog.refresh() {
        tracing::warn!(
            runtime_id,
            %err,
            "COREDUMP: failed to refresh model catalog before replay"
        );
    }
    let target = ctx
        .model_catalog
        .resolve_load_target(&selector)
        .map_err(|err| {
            (
                ControlErrorCode::LoadFailed,
                format!(
                    "failed to resolve runtime '{}' for replay: {}",
                    runtime_id, err
                ),
            )
        })?;

    activate_model_target(
        ctx.runtime_registry,
        ctx.resource_governor,
        ctx.session_registry,
        ctx.storage,
        ctx.model_catalog,
        &target,
    )
    .map(|loaded| loaded.runtime_id)
    .map_err(|err| (ControlErrorCode::LoadFailed, err.message().to_string()))
}

fn replay_session_prompt(manifest: &AgentCoreDumpManifest, branch_label: Option<&str>) -> String {
    let label = branch_label
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            manifest
                .session
                .as_ref()
                .map(|session| session.title.clone())
        })
        .unwrap_or_else(|| manifest.dump_id.clone());
    format!("[Replay] {label}")
}

fn replay_workload_label(
    ctx: &mut ProcessCommandContext<'_>,
    manifest: &AgentCoreDumpManifest,
) -> String {
    manifest
        .target
        .session_id
        .as_deref()
        .and_then(|session_id| {
            ctx.storage
                .latest_workload_for_session(session_id)
                .ok()
                .flatten()
        })
        .unwrap_or_else(|| "general".to_string())
}

fn build_replay_context_state(
    process: &CoreDumpProcessMetadata,
    patch: &CoreDumpReplayPatch,
) -> Result<(crate::process::ContextState, ReplayPatchSummary), String> {
    let mut state = process.context_state.clone();
    let mut summary = ReplayPatchSummary::default();

    if let Some(segments) = patch.context_segments.as_ref() {
        state.segments = parse_replay_segments(segments)?;
        summary.patched_context_segments = state.segments.len();
    }
    if let Some(segments) = patch.episodic_segments.as_ref() {
        state.episodic_segments = parse_replay_segments(segments)?;
        summary.patched_episodic_segments = state.episodic_segments.len();
    }
    summary.overridden_invocations = patch.tool_output_overrides.len();

    Ok((state, summary))
}

fn build_replay_tool_stubs(
    manifest: &AgentCoreDumpManifest,
    patch: &CoreDumpReplayPatch,
    tool_mode: ReplayToolMode,
) -> Result<VecDeque<ReplayToolStub>, String> {
    if !tool_mode.is_stubbed() {
        if patch.tool_output_overrides.is_empty() {
            return Ok(VecDeque::new());
        }
        return Err("tool_output_overrides require tool_mode='stubbed_recorded_tools'".to_string());
    }

    let mut overrides = patch
        .tool_output_overrides
        .iter()
        .map(|override_entry| (override_entry.tool_call_id.as_str(), override_entry))
        .collect::<HashMap<_, _>>();
    let mut stubs = VecDeque::new();

    for invocation in manifest.tool_invocation_history.iter() {
        let Some(mut outcome) = recorded_tool_outcome(invocation) else {
            continue;
        };
        if let Some(override_entry) = overrides.remove(invocation.tool_call_id.as_str()) {
            apply_tool_output_override(&mut outcome, override_entry);
        }
        stubs.push_back(ReplayToolStub {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.tool_name.clone(),
            command_text: invocation.command_text.clone(),
            outcome,
        });
    }

    if let Some(unknown) = overrides.keys().next() {
        return Err(format!(
            "tool_output_overrides references unknown tool_call_id '{}'",
            unknown
        ));
    }

    Ok(stubs)
}

fn recorded_tool_outcome(invocation: &CoreDumpToolInvocation) -> Option<SysCallOutcome> {
    let should_kill_process = invocation.kill || invocation.status == "killed";
    let success = match invocation.status.as_str() {
        "completed" => true,
        "failed" | "killed" => false,
        _ => return None,
    };

    Some(SysCallOutcome {
        output: render_tool_output_text(
            invocation.output_text.as_deref(),
            invocation.output.as_ref(),
        ),
        success,
        duration_ms: invocation.duration_ms.unwrap_or(0),
        should_kill_process,
        output_json: invocation.output.clone(),
        warnings: invocation.warnings.clone(),
        error_kind: invocation.error_kind.clone(),
        effects: invocation.effects.clone(),
    })
}

fn render_tool_output_text(output_text: Option<&str>, output: Option<&Value>) -> String {
    output_text
        .map(ToString::to_string)
        .or_else(|| output.map(stringify_json_value))
        .unwrap_or_default()
}

fn stringify_json_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| String::new()),
    }
}

fn apply_tool_output_override(
    outcome: &mut SysCallOutcome,
    override_entry: &CoreDumpReplayToolOutputOverride,
) {
    if let Some(output) = override_entry.output.as_ref() {
        outcome.output_json = Some(output.clone());
        if override_entry.output_text.is_none() {
            outcome.output = stringify_json_value(output);
        }
    }
    if let Some(output_text) = override_entry.output_text.as_ref() {
        outcome.output = output_text.clone();
    }
    if let Some(warnings) = override_entry.warnings.as_ref() {
        outcome.warnings = warnings.clone();
    }
    if let Some(error_kind) = override_entry.error_kind.as_ref() {
        outcome.error_kind = Some(error_kind.clone());
    }
    if let Some(effects) = override_entry.effects.as_ref() {
        outcome.effects = effects.clone();
    }
    if let Some(duration_ms) = override_entry.duration_ms {
        outcome.duration_ms = duration_ms;
    }
    if let Some(kill) = override_entry.kill {
        outcome.should_kill_process = kill;
    }
    if let Some(success) = override_entry.success {
        outcome.success = success;
    }
    if let Some(error_text) = override_entry.error_text.as_ref() {
        if !error_text.is_empty() {
            outcome.output = error_text.clone();
        }
        if outcome.error_kind.is_none() {
            outcome.error_kind = Some("counterfactual_override".to_string());
        }
    }
}

fn parse_replay_segments(
    segments: &[CoreDumpReplaySegmentPatch],
) -> Result<Vec<ContextSegment>, String> {
    segments
        .iter()
        .map(|segment| {
            let Some(kind) = ContextSegmentKind::parse(&segment.kind) else {
                return Err(format!(
                    "unsupported replay segment kind '{}'",
                    segment.kind
                ));
            };
            Ok(ContextSegment::new(kind, 0, segment.text.clone()))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn apply_replay_process_snapshot(
    ctx: &mut ProcessCommandContext<'_>,
    runtime_id: &str,
    pid: u64,
    process: &CoreDumpProcessMetadata,
    context_state: crate::process::ContextState,
    initial_state: ProcessState,
    tool_mode: ReplayToolMode,
    stubbed_invocations: VecDeque<ReplayToolStub>,
) -> Result<(), String> {
    let Some(engine) = ctx.runtime_registry.engine_mut(runtime_id) else {
        return Err(format!(
            "runtime '{}' is not available to finalize replay branch",
            runtime_id
        ));
    };
    let replay_process = engine
        .processes
        .get_mut(&pid)
        .ok_or_else(|| format!("replay process '{}' is missing from runtime", pid))?;
    replay_process.replace_debug_context_state(
        context_state,
        process.resident_prompt_checkpoint_bytes,
        process.turn_start_index,
    )?;
    replay_process.pending_human_request =
        if matches!(initial_state, ProcessState::WaitingForHumanInput) {
            process.pending_human_request.clone()
        } else {
            None
        };
    replay_process.termination_reason = process.termination_reason.clone();
    replay_process.state = initial_state.clone();

    ctx.scheduler.record_replay_process(
        pid,
        ReplayProcessMetadata {
            tool_mode,
            stubbed_invocations,
        },
    );

    Ok(())
}

fn normalize_replay_process_state(state: &str, has_pending_human_request: bool) -> ProcessState {
    match state {
        "WaitingForInput" => ProcessState::WaitingForInput,
        "WaitingForHumanInput" if has_pending_human_request => ProcessState::WaitingForHumanInput,
        "AwaitingTurnDecision" => ProcessState::AwaitingTurnDecision,
        _ => ProcessState::Ready,
    }
}

fn replay_mode_label(tool_mode: ReplayToolMode, patch_summary: &ReplayPatchSummary) -> String {
    if tool_mode.is_stubbed()
        || patch_summary.patched_context_segments > 0
        || patch_summary.patched_episodic_segments > 0
        || patch_summary.overridden_invocations > 0
    {
        "isolated_counterfactual_branch".to_string()
    } else {
        "isolated_relaunch".to_string()
    }
}

fn resolve_replay_tool_mode(
    raw: Option<&str>,
) -> Result<ReplayToolMode, (ControlErrorCode, String)> {
    match raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("stubbed_recorded_tools")
    {
        "stubbed_recorded_tools" | "stubbed" => Ok(ReplayToolMode::StubbedRecorded),
        "safe_filtered_live_tools" | "live_safe_filtered" => Ok(ReplayToolMode::LiveSafeFiltered),
        other => Err((
            ControlErrorCode::CoreDumpReplayInvalid,
            format!("unsupported replay tool_mode '{}'", other),
        )),
    }
}

fn seed_replay_history(
    ctx: &mut ProcessCommandContext<'_>,
    session_id: &str,
    pid: u64,
    workload_label: &str,
    manifest: &AgentCoreDumpManifest,
) -> Result<(), String> {
    let replay_messages = manifest
        .replay_messages
        .iter()
        .map(|message| StoredReplayMessage {
            role: message.role.clone(),
            kind: message.kind.clone(),
            content: message.content.clone(),
        })
        .collect::<Vec<_>>();
    ctx.storage
        .import_replay_history(
            session_id,
            pid,
            workload_label,
            "core_dump_replay_import",
            &replay_messages,
        )
        .map_err(|err| {
            format!(
                "failed to seed replay history for session '{}': {}",
                session_id, err
            )
        })
}

fn cleanup_failed_replay(
    ctx: &mut ProcessCommandContext<'_>,
    runtime_id: &str,
    session_id: &str,
    pid: u64,
) {
    if let Some(engine) = ctx.runtime_registry.engine_mut(runtime_id) {
        kill_managed_process_with_session(
            engine,
            ctx.memory,
            ctx.scheduler,
            ctx.session_registry,
            ctx.storage,
            pid,
            "replay_import_failed",
        );
    }
    let _ = ctx.runtime_registry.release_pid(ctx.storage, pid);
    let _ = ctx.session_registry.delete_session(ctx.storage, session_id);
}

fn first_user_prompt(manifest: &AgentCoreDumpManifest) -> Option<String> {
    manifest
        .replay_messages
        .iter()
        .find(|message| message.role == "user")
        .map(|message| message.content.clone())
}

fn parse_tool_caller(raw: &str) -> ToolCaller {
    match raw {
        "agent_supervisor" => ToolCaller::AgentSupervisor,
        "programmatic" => ToolCaller::Programmatic,
        "control_plane" => ToolCaller::ControlPlane,
        _ => ToolCaller::AgentText,
    }
}

fn process_state_label(state: &ProcessState) -> &'static str {
    match state {
        ProcessState::Ready => "Ready",
        ProcessState::Running => "Running",
        ProcessState::AwaitingTurnDecision => "AwaitingTurnDecision",
        ProcessState::WaitingForInput => "WaitingForInput",
        ProcessState::WaitingForHumanInput => "WaitingForHumanInput",
        ProcessState::Parked => "Parked",
        ProcessState::WaitingForSyscall => "WaitingForSyscall",
        ProcessState::Finished => "Finished",
    }
}

fn storage_err(
    code: ControlErrorCode,
) -> impl FnOnce(crate::storage::StorageError) -> (ControlErrorCode, String) {
    move |err| (code, err.to_string())
}

fn protocol_err(code: ControlErrorCode) -> impl FnOnce(String) -> (ControlErrorCode, String) {
    move |message| (code, message)
}
