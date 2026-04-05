use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agentic_control_models::{
    BackendCapabilitiesView, CoreDumpInfoResponse, CoreDumpListResponse, CoreDumpRequest,
    CoreDumpSummaryView, DiagnosticEvent,
};
use serde_json::{json, Value};

use super::models::{
    AgentCoreDumpManifest, CoreDumpAvailability, CoreDumpCaptureMetadata,
    CoreDumpGenerationMetadata, CoreDumpProcessMetadata, CoreDumpReplayMessage,
    CoreDumpRuntimeMetadata, CoreDumpSessionMetadata, CoreDumpTargetMetadata, CoreDumpTurnAssembly,
    WorkspaceEntry, WorkspaceEntryKind, WorkspaceSnapshot,
};
use super::package::{load_manifest_json, persist_manifest};
use super::{
    apply_core_dump_retention, configured_retention_policy, load_manifest_debug_checkpoints,
    load_manifest_tool_invocation_history, map_turn_assembly_snapshot, process_state_label,
};
use crate::checkpoint::snapshot_memory;
use crate::config::kernel_config;
use crate::process::AgentProcess;
use crate::runtime::TurnAssemblyStore;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::{CheckedOutProcessMetadata, ProcessScheduler, RestoredProcessMetadata};
use crate::session::SessionRegistry;
use crate::storage::{
    current_timestamp_ms, NewCoreDumpRecord, StorageService, StoredAuditEvent, StoredCoreDumpRecord,
};

const MANIFEST_FORMAT: &str = "agentic_core_dump.v1";
const DEFAULT_LIST_LIMIT: usize = 32;
const MAX_LIST_LIMIT: usize = 256;
const MAX_WORKSPACE_ENTRIES: usize = 4_096;

pub(crate) struct CaptureCoreDumpArgs<'a> {
    pub(crate) runtime_registry: &'a RuntimeRegistry,
    pub(crate) scheduler: &'a ProcessScheduler,
    pub(crate) session_registry: &'a SessionRegistry,
    pub(crate) storage: &'a mut StorageService,
    pub(crate) turn_assembly: &'a TurnAssemblyStore,
    pub(crate) memory: &'a crate::memory::NeuralMemory,
    pub(crate) in_flight: &'a HashSet<u64>,
}

pub(crate) fn capture_core_dump(
    args: CaptureCoreDumpArgs<'_>,
    request: CoreDumpRequest,
) -> Result<CoreDumpSummaryView, String> {
    let created_at_ms = current_timestamp_ms();
    let pid = resolve_target_pid(&request, args.session_registry)?;
    let session_id = resolve_session_id(args.session_registry, &request, pid)?;
    let runtime_id = args
        .runtime_registry
        .runtime_id_for_pid(pid)
        .map(ToString::to_string)
        .or_else(|| {
            session_id
                .as_deref()
                .and_then(|id| args.session_registry.runtime_id_for_session(id))
                .map(ToString::to_string)
        });
    let dump_id = build_dump_id(created_at_ms, session_id.as_deref(), Some(pid));
    let normalized = normalize_target_snapshot(
        args.runtime_registry,
        args.scheduler,
        args.turn_assembly,
        pid,
        runtime_id.as_deref(),
    )?;

    let include_workspace = request
        .include_workspace
        .unwrap_or(kernel_config().core_dump.include_workspace_by_default);
    let include_backend_state = request
        .include_backend_state
        .unwrap_or(kernel_config().core_dump.include_backend_state_by_default);
    let freeze_requested = request.freeze_target.unwrap_or(false);
    let reason = request
        .reason
        .clone()
        .or_else(|| request.mode.clone())
        .unwrap_or_else(|| "manual".to_string());
    let mode = request.mode.unwrap_or_else(|| "manual".to_string());

    let mut limitations = normalized.limitations;
    if freeze_requested {
        limitations.push("freeze_requested_but_not_implemented".to_string());
    }
    if include_workspace {
        limitations.push("workspace_snapshot_manifest_only".to_string());
    }
    limitations.push("logprobs_unavailable".to_string());
    if include_backend_state {
        limitations.push("backend_state_snapshot_unavailable".to_string());
    }

    let session = session_id
        .as_deref()
        .and_then(|id| args.session_registry.session(id))
        .map(|record| CoreDumpSessionMetadata {
            session_id: record.session_id.clone(),
            title: record.title.clone(),
            state: record.state.as_str().to_string(),
            active_pid: record.active_pid,
            runtime_id: record.runtime_id.clone(),
            created_at_ms: record.created_at_ms,
            updated_at_ms: record.updated_at_ms,
        });

    let replay_messages = session_id
        .as_deref()
        .map(|id| {
            args.storage
                .load_replay_messages_for_session(id)
                .map_err(|err| err.to_string())
        })
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .map(|message| CoreDumpReplayMessage {
            role: message.role,
            kind: message.kind,
            content: message.content,
        })
        .collect::<Vec<_>>();

    let session_audit_events = session_id
        .as_deref()
        .map(|id| {
            args.storage
                .recent_audit_events_for_session(
                    id,
                    kernel_config().core_dump.max_session_audit_events,
                )
                .map_err(|err| err.to_string())
        })
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .rev()
        .map(map_audit_event)
        .collect::<Vec<_>>();

    let tool_audit_lines = collect_tool_audit_lines(
        pid,
        kernel_config().core_dump.max_tool_audit_lines,
        workspace_audit_log_path(),
    )?;
    let debug_checkpoints = load_manifest_debug_checkpoints(args.storage, pid)?;
    let tool_invocation_history = load_manifest_tool_invocation_history(args.storage, pid)?;

    let workspace = if include_workspace {
        Some(capture_workspace_snapshot(
            &kernel_config().paths.workspace_dir,
            &kernel_config().core_dump.dump_dir,
        )?)
    } else {
        None
    };

    let manifest = AgentCoreDumpManifest {
        format: MANIFEST_FORMAT.to_string(),
        dump_id: dump_id.clone(),
        created_at_ms,
        capture: CoreDumpCaptureMetadata {
            mode,
            reason: reason.clone(),
            note: request.note.clone(),
            fidelity: normalized.fidelity.clone(),
            freeze_requested,
            freeze_applied: false,
            include_workspace,
            include_backend_state,
        },
        target: CoreDumpTargetMetadata {
            source: normalized.source,
            session_id: session_id.clone(),
            pid: Some(pid),
            runtime_id: normalized
                .runtime
                .as_ref()
                .map(|runtime| runtime.runtime_id.clone()),
            in_flight: args.in_flight.contains(&pid),
            state: normalized.state,
        },
        session,
        runtime: normalized.runtime,
        process: normalized.process,
        turn_assembly: normalized.turn_assembly,
        replay_messages,
        session_audit_events,
        tool_audit_lines,
        debug_checkpoints,
        tool_invocation_history,
        memory: snapshot_memory(args.memory),
        workspace,
        backend_state: include_backend_state.then_some(CoreDumpAvailability {
            available: false,
            detail: "backend interfaces do not expose snapshotable hidden state yet".to_string(),
        }),
        logprobs: Some(CoreDumpAvailability {
            available: false,
            detail: "backend interfaces do not expose token logprobs or probability trees"
                .to_string(),
        }),
        limitations,
    };

    let artifact = persist_manifest(&kernel_config().core_dump.dump_dir, &dump_id, &manifest)?;
    let record = StoredCoreDumpRecord {
        dump_id,
        created_at_ms,
        session_id,
        pid: Some(pid),
        reason,
        fidelity: manifest.capture.fidelity.clone(),
        path: artifact.path.display().to_string(),
        bytes: artifact.bytes,
        sha256: artifact.sha256,
        note: request.note,
    };
    args.storage
        .record_core_dump(&NewCoreDumpRecord {
            dump_id: record.dump_id.clone(),
            created_at_ms: record.created_at_ms,
            session_id: record.session_id.clone(),
            pid: record.pid,
            reason: record.reason.clone(),
            fidelity: record.fidelity.clone(),
            path: record.path.clone(),
            bytes: record.bytes,
            sha256: record.sha256.clone(),
            note: record.note.clone(),
        })
        .map_err(|err| err.to_string())?;
    let _ = apply_core_dump_retention(args.storage, configured_retention_policy()).map_err(|err| {
        tracing::warn!(%err, "COREDUMP: retention sweep failed after capture");
        err
    });

    Ok(map_summary(record))
}

pub(crate) fn list_core_dumps(
    storage: &StorageService,
    limit: Option<usize>,
) -> Result<CoreDumpListResponse, String> {
    let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT);
    let dumps = storage
        .load_core_dump_index(limit)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(map_summary)
        .collect();
    Ok(CoreDumpListResponse { dumps })
}

pub(crate) fn load_core_dump_info(
    storage: &StorageService,
    dump_id: &str,
) -> Result<Option<CoreDumpInfoResponse>, String> {
    let Some(record) = storage
        .core_dump_record(dump_id)
        .map_err(|err| err.to_string())?
    else {
        return Ok(None);
    };

    let manifest_json = load_manifest_json(Path::new(&record.path))?;
    Ok(Some(CoreDumpInfoResponse {
        dump: map_summary(record),
        manifest_json,
    }))
}

struct NormalizedTargetSnapshot {
    source: String,
    fidelity: String,
    state: String,
    runtime: Option<CoreDumpRuntimeMetadata>,
    process: Option<CoreDumpProcessMetadata>,
    turn_assembly: Option<CoreDumpTurnAssembly>,
    limitations: Vec<String>,
}

fn normalize_target_snapshot(
    runtime_registry: &RuntimeRegistry,
    scheduler: &ProcessScheduler,
    turn_assembly: &TurnAssemblyStore,
    pid: u64,
    runtime_id: Option<&str>,
) -> Result<NormalizedTargetSnapshot, String> {
    if let Some(runtime_id) = runtime_id {
        if let Some(engine) = runtime_registry.engine(runtime_id) {
            if let Some(process) = engine.processes.get(&pid) {
                return Ok(normalize_live_process(
                    runtime_registry,
                    turn_assembly,
                    runtime_id,
                    pid,
                    process,
                ));
            }
        }
    }

    if let Some(metadata) = scheduler.checked_out_process(pid) {
        return Ok(normalize_checked_out_process(
            runtime_registry,
            turn_assembly,
            runtime_id,
            pid,
            metadata,
        ));
    }

    if let Some(metadata) = scheduler.restored_process(pid) {
        return Ok(normalize_restored_process(
            runtime_registry,
            runtime_id,
            metadata,
        ));
    }

    Err(format!("pid {pid} is not available for core dump capture"))
}

fn normalize_live_process(
    runtime_registry: &RuntimeRegistry,
    turn_assembly: &TurnAssemblyStore,
    runtime_id: &str,
    pid: u64,
    process: &AgentProcess,
) -> NormalizedTargetSnapshot {
    let rendered_prompt = turn_assembly.render_inference_prompt(
        pid,
        process.prompt_text(),
        process.resident_prompt_checkpoint_bytes(),
    );
    NormalizedTargetSnapshot {
        source: "live_process".to_string(),
        fidelity: "full_context_snapshot".to_string(),
        state: process_state_label(&process.state).to_string(),
        runtime: runtime_metadata(runtime_registry, runtime_id),
        process: Some(CoreDumpProcessMetadata {
            owner_id: process.owner_id,
            tool_caller: process.tool_caller.as_str().to_string(),
            permission_policy: process.permission_policy.clone(),
            token_count: process.tokens.len(),
            tokens: process.tokens.clone(),
            index_pos: process.index_pos,
            turn_start_index: process.turn_start_index,
            max_tokens: process.max_tokens,
            context_slot_id: process.context_slot_id,
            resident_slot_policy: process.resident_slot_policy_label(),
            resident_slot_state: process.resident_slot_state_label(),
            resident_slot_snapshot_path: process
                .resident_slot_snapshot_path()
                .map(|path| path.display().to_string()),
            backend_id: Some(process.model.backend_id().to_string()),
            backend_class: Some(process.model.backend_class().as_str().to_string()),
            backend_capabilities: Some(process.model.backend_capabilities().into()),
            prompt_text: process.prompt_text().to_string(),
            resident_prompt_checkpoint_bytes: process.resident_prompt_checkpoint_bytes(),
            rendered_inference_prompt: rendered_prompt.full_prompt,
            resident_prompt_suffix: rendered_prompt.resident_prompt_suffix,
            generation: Some(CoreDumpGenerationMetadata {
                temperature: process.generation.temperature,
                top_p: process.generation.top_p,
                seed: process.generation.seed,
                max_tokens: process.generation.max_tokens,
            }),
            context_policy: process.context_policy.clone(),
            context_state: process.context_state.clone(),
            pending_human_request: process.pending_human_request.clone(),
            termination_reason: process.termination_reason.clone(),
        }),
        turn_assembly: turn_assembly.snapshot(pid).map(map_turn_assembly_snapshot),
        limitations: Vec::new(),
    }
}

fn normalize_checked_out_process(
    runtime_registry: &RuntimeRegistry,
    turn_assembly: &TurnAssemblyStore,
    runtime_id: Option<&str>,
    pid: u64,
    metadata: &CheckedOutProcessMetadata,
) -> NormalizedTargetSnapshot {
    let rendered_prompt = turn_assembly.render_inference_prompt(
        pid,
        &metadata.prompt_text,
        metadata.resident_prompt_checkpoint_bytes,
    );
    NormalizedTargetSnapshot {
        source: "checked_out_process".to_string(),
        fidelity: "full_context_snapshot".to_string(),
        state: metadata.state.clone(),
        runtime: runtime_id.and_then(|id| runtime_metadata(runtime_registry, id)),
        process: Some(CoreDumpProcessMetadata {
            owner_id: metadata.owner_id,
            tool_caller: metadata.tool_caller.as_str().to_string(),
            permission_policy: metadata.permission_policy.clone(),
            token_count: metadata.token_count,
            tokens: metadata.tokens.clone(),
            index_pos: metadata.index_pos,
            turn_start_index: metadata.turn_start_index,
            max_tokens: metadata.max_tokens,
            context_slot_id: metadata.context_slot_id,
            resident_slot_policy: metadata.resident_slot_policy.clone(),
            resident_slot_state: metadata.resident_slot_state.clone(),
            resident_slot_snapshot_path: metadata.resident_slot_snapshot_path.clone(),
            backend_id: metadata.backend_id.clone(),
            backend_class: metadata.backend_class.clone(),
            backend_capabilities: metadata
                .backend_capabilities
                .map(BackendCapabilitiesView::from),
            prompt_text: metadata.prompt_text.clone(),
            resident_prompt_checkpoint_bytes: metadata.resident_prompt_checkpoint_bytes,
            rendered_inference_prompt: rendered_prompt.full_prompt,
            resident_prompt_suffix: rendered_prompt.resident_prompt_suffix,
            generation: None,
            context_policy: metadata.context_policy.clone(),
            context_state: metadata.context_state.clone(),
            pending_human_request: metadata.pending_human_request.clone(),
            termination_reason: metadata.termination_reason.clone(),
        }),
        turn_assembly: turn_assembly.snapshot(pid).map(map_turn_assembly_snapshot),
        limitations: vec!["checked_out_snapshot_captured_before_worker_completion".to_string()],
    }
}

fn normalize_restored_process(
    runtime_registry: &RuntimeRegistry,
    runtime_id: Option<&str>,
    metadata: &RestoredProcessMetadata,
) -> NormalizedTargetSnapshot {
    NormalizedTargetSnapshot {
        source: "restored_process_metadata".to_string(),
        fidelity: "restore_metadata_only".to_string(),
        state: metadata.state.clone(),
        runtime: runtime_id.and_then(|id| runtime_metadata(runtime_registry, id)),
        process: Some(CoreDumpProcessMetadata {
            owner_id: metadata.owner_id,
            tool_caller: metadata.tool_caller.as_str().to_string(),
            permission_policy: metadata.permission_policy.clone(),
            token_count: metadata.token_count,
            tokens: Vec::new(),
            index_pos: 0,
            turn_start_index: 0,
            max_tokens: metadata.max_tokens,
            context_slot_id: metadata.context_slot_id,
            resident_slot_policy: metadata.resident_slot_policy.clone(),
            resident_slot_state: metadata.resident_slot_state.clone(),
            resident_slot_snapshot_path: metadata.resident_slot_snapshot_path.clone(),
            backend_id: metadata.backend_id.clone(),
            backend_class: metadata.backend_class.clone(),
            backend_capabilities: metadata
                .backend_capabilities
                .map(BackendCapabilitiesView::from),
            prompt_text: String::new(),
            resident_prompt_checkpoint_bytes: 0,
            rendered_inference_prompt: String::new(),
            resident_prompt_suffix: String::new(),
            generation: None,
            context_policy: metadata.context_policy.clone(),
            context_state: metadata.context_state.clone(),
            pending_human_request: metadata.pending_human_request.clone(),
            termination_reason: None,
        }),
        turn_assembly: None,
        limitations: vec![
            "restored_process_missing_prompt_buffer".to_string(),
            "restored_process_missing_token_buffer".to_string(),
        ],
    }
}

fn resolve_target_pid(
    request: &CoreDumpRequest,
    session_registry: &SessionRegistry,
) -> Result<u64, String> {
    match (request.pid, request.session_id.as_deref()) {
        (Some(pid), Some(session_id)) => {
            let bound = session_registry.session_id_for_pid(pid);
            if bound != Some(session_id) {
                return Err(format!("pid {pid} is not bound to session '{session_id}'"));
            }
            Ok(pid)
        }
        (Some(pid), None) => Ok(pid),
        (None, Some(session_id)) => session_registry
            .active_pid_for_session(session_id)
            .ok_or_else(|| format!("session '{session_id}' has no active pid")),
        (None, None) => Err("core dump request requires either pid or session_id".to_string()),
    }
}

fn resolve_session_id(
    session_registry: &SessionRegistry,
    request: &CoreDumpRequest,
    pid: u64,
) -> Result<Option<String>, String> {
    if let Some(session_id) = request.session_id.as_ref() {
        return Ok(Some(session_id.clone()));
    }
    Ok(session_registry
        .session_id_for_pid(pid)
        .map(ToString::to_string))
}

fn runtime_metadata(
    runtime_registry: &RuntimeRegistry,
    runtime_id: &str,
) -> Option<CoreDumpRuntimeMetadata> {
    let descriptor = runtime_registry.descriptor(runtime_id)?;
    Some(CoreDumpRuntimeMetadata {
        runtime_id: descriptor.runtime_id.clone(),
        target_kind: descriptor.target_kind.clone(),
        logical_model_id: descriptor.logical_model_id.clone(),
        display_path: descriptor.display_path.clone(),
        backend_id: descriptor.backend_id.clone(),
        backend_class: descriptor.backend_class.as_str().to_string(),
        provider_id: descriptor.provider_id.clone(),
        remote_model_id: descriptor.remote_model_id.clone(),
        load_mode: descriptor.load_mode.clone(),
        reservation_ram_bytes: descriptor.reservation_ram_bytes,
        reservation_vram_bytes: descriptor.reservation_vram_bytes,
        pinned: descriptor.pinned,
        transition_state: descriptor.transition_state.clone(),
        loaded: runtime_registry.is_runtime_loaded(runtime_id),
    })
}

fn map_audit_event(event: StoredAuditEvent) -> DiagnosticEvent {
    DiagnosticEvent {
        category: event.category,
        kind: event.kind,
        title: event.title,
        detail: event.detail,
        recorded_at_ms: event.recorded_at_ms,
        session_id: event.session_id,
        pid: event.pid,
        runtime_id: event.runtime_id,
    }
}

fn workspace_audit_log_path() -> PathBuf {
    kernel_config()
        .paths
        .workspace_dir
        .join(&kernel_config().tools.audit_log_file)
}

fn collect_tool_audit_lines(pid: u64, limit: usize, path: PathBuf) -> Result<Vec<Value>, String> {
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(format!("read tool audit log '{}': {err}", path.display()));
        }
    };

    let mut matched = Vec::new();
    for line in content.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str::<Value>(line).unwrap_or_else(|_| json!({ "raw": line }));
        if value.get("pid").and_then(Value::as_u64) == Some(pid) {
            matched.push(value);
            if matched.len() >= limit {
                break;
            }
        }
    }
    matched.reverse();
    Ok(matched)
}

fn capture_workspace_snapshot(
    workspace_root: &Path,
    dump_dir: &Path,
) -> Result<WorkspaceSnapshot, String> {
    let mut entries = Vec::new();
    let mut total_entries = 0usize;
    let mut total_bytes = 0u64;
    let mut truncated = false;
    let mut skipped_roots = Vec::new();
    if dump_dir.starts_with(workspace_root) {
        skipped_roots.push(relative_display_path(workspace_root, dump_dir));
    }
    visit_workspace_dir(
        workspace_root,
        workspace_root,
        dump_dir,
        &mut entries,
        &mut total_entries,
        &mut total_bytes,
        &mut truncated,
    )?;
    Ok(WorkspaceSnapshot {
        root: workspace_root.display().to_string(),
        skipped_roots,
        total_entries,
        total_bytes,
        truncated,
        entries,
    })
}

fn visit_workspace_dir(
    root: &Path,
    dir: &Path,
    dump_dir: &Path,
    entries: &mut Vec<WorkspaceEntry>,
    total_entries: &mut usize,
    total_bytes: &mut u64,
    truncated: &mut bool,
) -> Result<(), String> {
    if *truncated {
        return Ok(());
    }

    let mut dir_entries = fs::read_dir(dir)
        .map_err(|err| format!("read workspace directory '{}': {err}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("enumerate workspace directory '{}': {err}", dir.display()))?;
    dir_entries.sort_by_key(|entry| entry.path());

    for entry in dir_entries {
        if *truncated {
            break;
        }

        let path = entry.path();
        if path.starts_with(dump_dir) {
            continue;
        }

        let metadata = fs::symlink_metadata(&path)
            .map_err(|err| format!("stat workspace path '{}': {err}", path.display()))?;
        let kind = if metadata.is_file() {
            WorkspaceEntryKind::File
        } else if metadata.is_dir() {
            WorkspaceEntryKind::Directory
        } else if metadata.file_type().is_symlink() {
            WorkspaceEntryKind::Symlink
        } else {
            WorkspaceEntryKind::Other
        };

        *total_entries = total_entries.saturating_add(1);
        if entries.len() >= MAX_WORKSPACE_ENTRIES {
            *truncated = true;
            break;
        }

        let bytes = metadata.is_file().then_some(metadata.len());
        if let Some(bytes) = bytes {
            *total_bytes = total_bytes.saturating_add(bytes);
        }
        entries.push(WorkspaceEntry {
            path: relative_display_path(root, &path),
            kind,
            bytes,
            modified_at_ms: modified_at_ms(&metadata),
        });

        if metadata.is_dir() {
            visit_workspace_dir(
                root,
                &path,
                dump_dir,
                entries,
                total_entries,
                total_bytes,
                truncated,
            )?;
        }
    }

    Ok(())
}

fn relative_display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .unwrap_or(path)
        .display()
        .to_string()
}

fn modified_at_ms(metadata: &fs::Metadata) -> Option<u128> {
    metadata
        .modified()
        .ok()
        .and_then(|timestamp| timestamp.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
}

fn build_dump_id(created_at_ms: i64, session_id: Option<&str>, pid: Option<u64>) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or(0);
    let session = session_id
        .map(sanitize_fragment)
        .unwrap_or_else(|| "sessionless".to_string());
    let pid = pid
        .map(|pid| format!("pid{pid}"))
        .unwrap_or_else(|| "nopid".to_string());
    format!("core-{created_at_ms}-{session}-{pid}-{nanos:09}")
}

fn sanitize_fragment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars().take(48) {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "anon".to_string()
    } else {
        out
    }
}

fn map_summary(record: StoredCoreDumpRecord) -> CoreDumpSummaryView {
    CoreDumpSummaryView {
        dump_id: record.dump_id,
        created_at_ms: record.created_at_ms,
        session_id: record.session_id,
        pid: record.pid,
        reason: record.reason,
        fidelity: record.fidelity,
        path: record.path,
        bytes: record.bytes,
        sha256: record.sha256,
        note: record.note,
    }
}
