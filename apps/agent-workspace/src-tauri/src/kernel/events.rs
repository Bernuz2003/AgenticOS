use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use agentic_control_models::{
    DiagnosticEvent as KernelDiagnosticEvent, KernelEvent, KernelEventEnvelope, SubscribeResult,
};
use agentic_protocol::{HelloRequest, OpCode, PROTOCOL_VERSION_V1};
use tauri::{AppHandle, Emitter};

use super::auth::kernel_token_path;
use super::client::{transport, KernelBridge};
use super::composer;
use super::live_timeline;
use super::live_timeline::TimelineStore;
use crate::models::kernel::{AuditEvent, LobbySnapshot};

const BRIDGE_STATUS_EVENT: &str = "kernel://bridge_status";
const LOBBY_SNAPSHOT_EVENT: &str = "kernel://lobby_snapshot";
const WORKSPACE_SNAPSHOT_EVENT: &str = "kernel://workspace_snapshot";
const TIMELINE_SNAPSHOT_EVENT: &str = "kernel://timeline_snapshot";
const DIAGNOSTIC_EVENT: &str = "kernel://diagnostic_event";

#[derive(Debug, Clone, serde::Serialize)]
pub struct BridgeStatusEvent {
    pub connected: bool,
    pub error: Option<String>,
}

pub fn spawn_event_bridge(
    app: AppHandle,
    kernel_addr: String,
    workspace_root: PathBuf,
    bridge: Arc<Mutex<KernelBridge>>,
    timeline_store: Arc<Mutex<TimelineStore>>,
) {
    thread::Builder::new()
        .name("tauri-kernel-events".into())
        .spawn(move || loop {
            let result = run_event_bridge(
                &app,
                &kernel_addr,
                &workspace_root,
                &bridge,
                &timeline_store,
            );

            if let Err(err) = result {
                emit_bridge_status(&app, false, Some(err));
                thread::sleep(Duration::from_millis(900));
            }
        })
        .expect("failed to spawn tauri kernel event bridge");
}

fn run_event_bridge(
    app: &AppHandle,
    kernel_addr: &str,
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<TimelineStore>>,
) -> Result<(), String> {
    let mut stream = TcpStream::connect(kernel_addr).map_err(|err| err.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|err| err.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| err.to_string())?;

    authenticate(&mut stream, workspace_root)?;
    negotiate_events_hello(&mut stream)?;
    subscribe(&mut stream)?;

    emit_bridge_status(app, true, None);
    emit_lobby_snapshot(app, bridge);

    let mut frame_buffer = Vec::new();
    let mut last_lobby_refresh = Instant::now() - Duration::from_secs(1);
    let mut last_workspace_refresh = std::collections::HashMap::<u64, Instant>::new();

    loop {
        match transport::read_stream_frame(&mut stream, &mut frame_buffer, Duration::from_secs(30))
            .map_err(|err| err.to_string())?
        {
            Some(frame) if frame.kind == "DATA" && frame.code.eq_ignore_ascii_case("event") => {
                let event = transport::decode_protocol_data::<KernelEventEnvelope>(&frame.payload)
                    .map_err(|err| err.to_string())?;
                handle_kernel_event(
                    app,
                    bridge,
                    timeline_store,
                    workspace_root,
                    event.event,
                    &mut last_lobby_refresh,
                    &mut last_workspace_refresh,
                );
            }
            Some(frame) if frame.kind == "-ERR" => {
                return Err(
                    transport::decode_protocol_error(&frame.code, &frame.payload).to_string(),
                );
            }
            Some(_) | None => continue,
        }
    }
}

fn handle_kernel_event(
    app: &AppHandle,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    workspace_root: &Path,
    event: KernelEvent,
    last_lobby_refresh: &mut Instant,
    last_workspace_refresh: &mut std::collections::HashMap<u64, Instant>,
) {
    match &event {
        KernelEvent::CoreDumpCreated { pid, .. } => {
            maybe_emit_workspace_snapshot(
                app,
                workspace_root,
                bridge,
                timeline_store,
                *pid,
                last_workspace_refresh,
                true,
            );
            maybe_emit_lobby_snapshot(app, bridge, last_lobby_refresh, false);
        }
        KernelEvent::LobbyChanged { .. } | KernelEvent::ModelChanged { .. } => {
            maybe_emit_lobby_snapshot(app, bridge, last_lobby_refresh, false);
        }
        KernelEvent::WorkspaceChanged { pid, .. } => {
            maybe_emit_workspace_snapshot(
                app,
                workspace_root,
                bridge,
                timeline_store,
                *pid,
                last_workspace_refresh,
                false,
            );
        }
        KernelEvent::SessionStarted { pid, .. } => {
            if let Ok(mut store) = timeline_store.lock() {
                apply_timeline_store_event(&mut store, &event);
            }
            emit_timeline_snapshot(app, bridge, timeline_store, workspace_root, *pid);
            maybe_emit_workspace_snapshot(
                app,
                workspace_root,
                bridge,
                timeline_store,
                *pid,
                last_workspace_refresh,
                true,
            );
            maybe_emit_lobby_snapshot(app, bridge, last_lobby_refresh, true);
        }
        KernelEvent::TimelineSegment { pid, .. } => {
            if let Ok(mut store) = timeline_store.lock() {
                apply_timeline_store_event(&mut store, &event);
            }
            emit_timeline_snapshot(app, bridge, timeline_store, workspace_root, *pid);
            maybe_emit_workspace_snapshot(
                app,
                workspace_root,
                bridge,
                timeline_store,
                *pid,
                last_workspace_refresh,
                false,
            );
        }
        KernelEvent::InvocationUpdated { pid, .. } => {
            if let Ok(mut store) = timeline_store.lock() {
                apply_timeline_store_event(&mut store, &event);
            }
            emit_timeline_snapshot(app, bridge, timeline_store, workspace_root, *pid);
            maybe_emit_workspace_snapshot(
                app,
                workspace_root,
                bridge,
                timeline_store,
                *pid,
                last_workspace_refresh,
                false,
            );
        }
        KernelEvent::SessionFinished { pid, reason, .. } => {
            if let Ok(mut store) = timeline_store.lock() {
                apply_timeline_store_event(&mut store, &event);
            }
            emit_timeline_snapshot(app, bridge, timeline_store, workspace_root, *pid);
            if should_evict_live_timeline(reason) {
                if let Ok(mut store) = timeline_store.lock() {
                    store.evict_session(*pid);
                }
            }
            maybe_emit_workspace_snapshot(
                app,
                workspace_root,
                bridge,
                timeline_store,
                *pid,
                last_workspace_refresh,
                true,
            );
            maybe_emit_lobby_snapshot(app, bridge, last_lobby_refresh, true);
        }
        KernelEvent::SessionErrored { pid, .. } => {
            if let Ok(mut store) = timeline_store.lock() {
                apply_timeline_store_event(&mut store, &event);
            }
            emit_timeline_snapshot(app, bridge, timeline_store, workspace_root, *pid);
            if let Ok(mut store) = timeline_store.lock() {
                store.evict_session(*pid);
            }
            maybe_emit_workspace_snapshot(
                app,
                workspace_root,
                bridge,
                timeline_store,
                *pid,
                last_workspace_refresh,
                true,
            );
            maybe_emit_lobby_snapshot(app, bridge, last_lobby_refresh, true);
        }
        KernelEvent::DiagnosticRecorded { event } => {
            emit_diagnostic_event(app, &event);
            if matches!(event.category.as_str(), "runtime" | "admission" | "kernel") {
                maybe_emit_lobby_snapshot(app, bridge, last_lobby_refresh, false);
            }
        }
        KernelEvent::KernelShutdownRequested => {
            emit_bridge_status(app, false, None);
        }
    }
}

pub(crate) fn apply_timeline_store_event(store: &mut TimelineStore, event: &KernelEvent) {
    match event {
        KernelEvent::CoreDumpCreated { .. } => {}
        KernelEvent::SessionStarted {
            session_id,
            pid,
            workload,
            prompt,
        } => {
            store.insert_started_session(*pid, session_id.clone(), prompt.clone(), workload.clone())
        }
        KernelEvent::TimelineSegment {
            pid,
            segment_kind,
            text,
        } => store.append_timeline_segment(*pid, segment_kind.clone(), text),
        KernelEvent::InvocationUpdated { pid, invocation } => {
            store.upsert_invocation(*pid, invocation.clone());
        }
        KernelEvent::SessionFinished {
            pid,
            tokens_generated,
            elapsed_secs,
            reason,
        } => {
            if reason == "turn_completed" || reason == "awaiting_turn_decision" {
                store.finish_session_with_reason(*pid, None, None);
            } else {
                store.finish_session_with_reason(
                    *pid,
                    match (tokens_generated, elapsed_secs) {
                        (Some(tokens_generated), Some(elapsed_secs)) => {
                            Some(live_timeline::ProcessFinishedMarker {
                                pid: *pid,
                                tokens_generated: *tokens_generated,
                                elapsed_secs: *elapsed_secs,
                            })
                        }
                        _ => None,
                    },
                    if reason == "completed" {
                        None
                    } else {
                        Some(reason.as_str())
                    },
                );
            }
        }
        KernelEvent::SessionErrored { pid, message } => store.set_error(*pid, message.clone()),
        _ => {}
    }
}

fn maybe_emit_lobby_snapshot(
    app: &AppHandle,
    bridge: &Arc<Mutex<KernelBridge>>,
    last_refresh: &mut Instant,
    force: bool,
) {
    if !force && last_refresh.elapsed() < Duration::from_millis(250) {
        return;
    }

    *last_refresh = Instant::now();
    emit_lobby_snapshot(app, bridge);
}

fn emit_lobby_snapshot(app: &AppHandle, bridge: &Arc<Mutex<KernelBridge>>) {
    let snapshot = match bridge.lock() {
        Ok(mut bridge) => bridge.fetch_lobby_snapshot(),
        Err(_) => LobbySnapshot {
            connected: false,
            selected_model_id: String::new(),
            loaded_model_id: String::new(),
            loaded_target_kind: None,
            loaded_provider_id: None,
            loaded_remote_model_id: None,
            loaded_backend_id: None,
            loaded_backend_class: None,
            loaded_backend_capabilities: None,
            global_accounting: None,
            loaded_backend_telemetry: None,
            loaded_remote_model: None,
            memory: None,
            runtime_instances: Vec::new(),
            managed_local_runtimes: Vec::new(),
            resource_governor: None,
            runtime_load_queue: Vec::new(),
            mcp: None,
            global_audit_events: Vec::new(),
            scheduled_jobs: Vec::new(),
            orchestrations: Vec::new(),
            sessions: Vec::new(),
            error: Some("Bridge state lock poisoned".to_string()),
        },
    };
    let _ = app.emit(LOBBY_SNAPSHOT_EVENT, snapshot);
}

fn maybe_emit_workspace_snapshot(
    app: &AppHandle,
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    pid: u64,
    last_refresh: &mut std::collections::HashMap<u64, Instant>,
    force: bool,
) {
    let now = Instant::now();
    if !force
        && last_refresh
            .get(&pid)
            .is_some_and(|previous| now.duration_since(*previous) < Duration::from_millis(180))
    {
        return;
    }

    last_refresh.insert(pid, now);
    if let Ok(snapshot) =
        composer::compose_workspace_snapshot_for_pid(workspace_root, bridge, timeline_store, pid)
    {
        let _ = app.emit(WORKSPACE_SNAPSHOT_EVENT, snapshot);
    }
}

fn emit_timeline_snapshot(
    app: &AppHandle,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    workspace_root: &Path,
    pid: u64,
) {
    let Ok(Some(timeline)) =
        composer::compose_timeline_snapshot_for_pid(workspace_root, bridge, timeline_store, pid)
    else {
        return;
    };
    let _ = app.emit(TIMELINE_SNAPSHOT_EVENT, timeline);
}

fn should_evict_live_timeline(reason: &str) -> bool {
    !matches!(reason, "turn_completed" | "awaiting_turn_decision")
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::should_evict_live_timeline;

    #[test]
    fn interactive_turn_finish_reasons_keep_live_timeline_attached() {
        assert!(!should_evict_live_timeline("turn_completed"));
        assert!(!should_evict_live_timeline("awaiting_turn_decision"));
    }

    #[test]
    fn terminal_finish_reasons_evict_live_timeline() {
        assert!(should_evict_live_timeline("completed"));
        assert!(should_evict_live_timeline("killed"));
        assert!(should_evict_live_timeline("worker_error"));
    }
}

fn emit_bridge_status(app: &AppHandle, connected: bool, error: Option<String>) {
    let _ = app.emit(BRIDGE_STATUS_EVENT, BridgeStatusEvent { connected, error });
}

fn emit_diagnostic_event(app: &AppHandle, event: &KernelDiagnosticEvent) {
    let _ = app.emit(DIAGNOSTIC_EVENT, map_diagnostic_event(event));
}

fn map_diagnostic_event(event: &KernelDiagnosticEvent) -> AuditEvent {
    AuditEvent {
        category: event.category.clone(),
        kind: event.kind.clone(),
        title: event.title.clone(),
        detail: event.detail.clone(),
        recorded_at_ms: event.recorded_at_ms,
        session_id: event.session_id.clone(),
        pid: event.pid,
        runtime_id: event.runtime_id.clone(),
    }
}

fn authenticate(stream: &mut TcpStream, workspace_root: &Path) -> Result<(), String> {
    let token = load_token(workspace_root)?;
    if token.is_empty() {
        return Ok(());
    }

    transport::send_command(stream, OpCode::Auth, "events", token.as_bytes())
        .map_err(|err| err.to_string())?;
    let response = transport::read_single_frame(stream, Duration::from_secs(5))
        .map_err(|err| err.to_string())?;
    if response.kind != "+OK" {
        return Err(
            transport::decode_protocol_error(&response.code, &response.payload).to_string(),
        );
    }

    Ok(())
}

fn negotiate_events_hello(stream: &mut TcpStream) -> Result<(), String> {
    let payload = serde_json::to_vec(&HelloRequest {
        supported_versions: vec![PROTOCOL_VERSION_V1.to_string()],
        required_capabilities: vec!["event_stream_v1".to_string()],
    })
    .map_err(|err| err.to_string())?;
    transport::send_command(stream, OpCode::Hello, "events", &payload)
        .map_err(|err| err.to_string())?;
    let response = transport::read_single_frame(stream, Duration::from_secs(5))
        .map_err(|err| err.to_string())?;
    if response.kind != "+OK" {
        return Err(
            transport::decode_protocol_error(&response.code, &response.payload).to_string(),
        );
    }
    Ok(())
}

fn subscribe(stream: &mut TcpStream) -> Result<(), String> {
    transport::send_command(stream, OpCode::Subscribe, "events", &[])
        .map_err(|err| err.to_string())?;
    let response = transport::read_single_frame(stream, Duration::from_secs(5))
        .map_err(|err| err.to_string())?;
    if response.kind != "+OK" {
        return Err(
            transport::decode_protocol_error(&response.code, &response.payload).to_string(),
        );
    }
    let _ = transport::decode_protocol_data::<SubscribeResult>(&response.payload)
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn load_token(workspace_root: &Path) -> Result<String, String> {
    let token_path = kernel_token_path(workspace_root);
    match fs::read_to_string(token_path) {
        Ok(token) => Ok(token.trim().to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err.to_string()),
    }
}
