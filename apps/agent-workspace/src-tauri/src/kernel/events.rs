use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use agentic_control_models::{KernelEvent, KernelEventEnvelope, SubscribeResult};
use agentic_protocol::{HelloRequest, OpCode, PROTOCOL_VERSION_V1};
use tauri::{AppHandle, Emitter};

use super::audit;
use super::auth::kernel_token_path;
use super::client::KernelBridge;
use super::protocol;
use super::stream::{augment_timeline_with_tool_results, TimelineStore};
use crate::models::kernel::LobbySnapshot;

const BRIDGE_STATUS_EVENT: &str = "kernel://bridge_status";
const LOBBY_SNAPSHOT_EVENT: &str = "kernel://lobby_snapshot";
const WORKSPACE_SNAPSHOT_EVENT: &str = "kernel://workspace_snapshot";
const TIMELINE_SNAPSHOT_EVENT: &str = "kernel://timeline_snapshot";

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
        match protocol::read_stream_frame(&mut stream, &mut frame_buffer, Duration::from_secs(30))
            .map_err(|err| err.to_string())?
        {
            Some(frame) if frame.kind == "DATA" && frame.code.eq_ignore_ascii_case("event") => {
                let event = protocol::decode_protocol_data::<KernelEventEnvelope>(&frame.payload)
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
                    protocol::decode_protocol_error(&frame.code, &frame.payload).to_string()
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
    match event {
        KernelEvent::LobbyChanged { .. } | KernelEvent::ModelChanged { .. } => {
            maybe_emit_lobby_snapshot(app, bridge, last_lobby_refresh, false);
        }
        KernelEvent::WorkspaceChanged { pid, .. } => {
            maybe_emit_workspace_snapshot(app, bridge, pid, last_workspace_refresh, false);
        }
        KernelEvent::SessionStarted {
            session_id,
            pid,
            workload,
            prompt,
        } => {
            if let Ok(mut store) = timeline_store.lock() {
                store.insert_started_session(pid, session_id, prompt, workload);
            }
            emit_timeline_snapshot(app, timeline_store, workspace_root, pid);
            maybe_emit_workspace_snapshot(app, bridge, pid, last_workspace_refresh, true);
            maybe_emit_lobby_snapshot(app, bridge, last_lobby_refresh, true);
        }
        KernelEvent::TimelineChunk { pid, text } => {
            if let Ok(mut store) = timeline_store.lock() {
                store.append_assistant_chunk(pid, &text);
            }
            emit_timeline_snapshot(app, timeline_store, workspace_root, pid);
            maybe_emit_workspace_snapshot(app, bridge, pid, last_workspace_refresh, false);
        }
        KernelEvent::SessionFinished {
            pid,
            tokens_generated,
            elapsed_secs,
            reason,
        } => {
            if let Ok(mut store) = timeline_store.lock() {
                if reason == "turn_completed" || reason == "awaiting_turn_decision" {
                    store.finish_session_with_reason(pid, None, None);
                } else {
                    store.finish_session_with_reason(
                        pid,
                        match (tokens_generated, elapsed_secs) {
                            (Some(tokens_generated), Some(elapsed_secs)) => {
                                Some(super::stream::ProcessFinishedMarker {
                                    pid,
                                    tokens_generated,
                                    elapsed_secs,
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
            emit_timeline_snapshot(app, timeline_store, workspace_root, pid);
            if let Ok(mut store) = timeline_store.lock() {
                store.evict_session(pid);
            }
            maybe_emit_workspace_snapshot(app, bridge, pid, last_workspace_refresh, true);
            maybe_emit_lobby_snapshot(app, bridge, last_lobby_refresh, true);
        }
        KernelEvent::SessionErrored { pid, message } => {
            if let Ok(mut store) = timeline_store.lock() {
                store.set_error(pid, message);
            }
            emit_timeline_snapshot(app, timeline_store, workspace_root, pid);
            if let Ok(mut store) = timeline_store.lock() {
                store.evict_session(pid);
            }
            maybe_emit_workspace_snapshot(app, bridge, pid, last_workspace_refresh, true);
            maybe_emit_lobby_snapshot(app, bridge, last_lobby_refresh, true);
        }
        KernelEvent::KernelShutdownRequested => {
            emit_bridge_status(app, false, None);
        }
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
            resource_governor: None,
            runtime_load_queue: Vec::new(),
            global_audit_events: Vec::new(),
            orchestrations: Vec::new(),
            sessions: Vec::new(),
            error: Some("Bridge state lock poisoned".to_string()),
        },
    };
    let _ = app.emit(LOBBY_SNAPSHOT_EVENT, snapshot);
}

fn maybe_emit_workspace_snapshot(
    app: &AppHandle,
    bridge: &Arc<Mutex<KernelBridge>>,
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
    if let Ok(mut bridge) = bridge.lock() {
        if let Ok(snapshot) = bridge.fetch_workspace_snapshot(pid) {
            let _ = app.emit(WORKSPACE_SNAPSHOT_EVENT, snapshot);
        }
    }
}

fn emit_timeline_snapshot(
    app: &AppHandle,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    workspace_root: &Path,
    pid: u64,
) {
    let timeline = match timeline_store.lock() {
        Ok(store) => store.snapshot(pid),
        Err(_) => None,
    };
    let Some(timeline) = timeline else {
        return;
    };
    let audit_entries = audit::read_recent_audit_entries_for_pid(workspace_root, pid, 32);
    let augmented = augment_timeline_with_tool_results(timeline, &audit_entries);
    let _ = app.emit(TIMELINE_SNAPSHOT_EVENT, augmented);
}

fn emit_bridge_status(app: &AppHandle, connected: bool, error: Option<String>) {
    let _ = app.emit(BRIDGE_STATUS_EVENT, BridgeStatusEvent { connected, error });
}

fn authenticate(stream: &mut TcpStream, workspace_root: &Path) -> Result<(), String> {
    let token = load_token(workspace_root)?;
    if token.is_empty() {
        return Ok(());
    }

    protocol::send_command(stream, OpCode::Auth, "events", token.as_bytes())
        .map_err(|err| err.to_string())?;
    let response = protocol::read_single_frame(stream, Duration::from_secs(5))
        .map_err(|err| err.to_string())?;
    if response.kind != "+OK" {
        return Err(protocol::decode_protocol_error(&response.code, &response.payload).to_string());
    }

    Ok(())
}

fn negotiate_events_hello(stream: &mut TcpStream) -> Result<(), String> {
    let payload = serde_json::to_vec(&HelloRequest {
        supported_versions: vec![PROTOCOL_VERSION_V1.to_string()],
        required_capabilities: vec!["event_stream_v1".to_string()],
    })
    .map_err(|err| err.to_string())?;
    protocol::send_command(stream, OpCode::Hello, "events", &payload)
        .map_err(|err| err.to_string())?;
    let response = protocol::read_single_frame(stream, Duration::from_secs(5))
        .map_err(|err| err.to_string())?;
    if response.kind != "+OK" {
        return Err(protocol::decode_protocol_error(&response.code, &response.payload).to_string());
    }
    Ok(())
}

fn subscribe(stream: &mut TcpStream) -> Result<(), String> {
    protocol::send_command(stream, OpCode::Subscribe, "events", &[])
        .map_err(|err| err.to_string())?;
    let response = protocol::read_single_frame(stream, Duration::from_secs(5))
        .map_err(|err| err.to_string())?;
    if response.kind != "+OK" {
        return Err(protocol::decode_protocol_error(&response.code, &response.payload).to_string());
    }
    let _ = protocol::decode_protocol_data::<SubscribeResult>(&response.payload)
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
