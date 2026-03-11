use std::collections::HashMap;
use std::fs;
use std::net::TcpStream;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agentic_control_models::ExecStartPayload;
use agentic_protocol::OpCode;
use serde::{Deserialize, Serialize};

use super::audit::AuditLogEntry;
use super::auth::kernel_token_path;
use super::protocol;
use crate::models::kernel::{
    StartSessionResult, TimelineItem, TimelineItemKind, TimelineSnapshot, WorkspaceSnapshot,
};

#[derive(Debug, Default)]
pub struct TimelineStore {
    sessions: HashMap<u64, TimelineSessionState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TimelineTurn {
    prompt: String,
    assistant_stream: String,
    running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TimelineSessionState {
    session_id: String,
    pid: u64,
    workload: String,
    turns: Vec<TimelineTurn>,
    error: Option<String>,
    system_events: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProcessFinishedMarker {
    pub(crate) pid: u64,
    pub(crate) tokens_generated: u64,
    pub(crate) elapsed_secs: f64,
}

impl TimelineStore {
    pub fn insert_started_session(&mut self, pid: u64, prompt: String, workload: String) {
        if let Some(session) = self.sessions.get_mut(&pid) {
            session.workload = workload;
            session.error = None;
            if session.turns.is_empty() {
                session.turns.push(TimelineTurn {
                    prompt,
                    assistant_stream: String::new(),
                    running: true,
                });
            }
            return;
        }

        let state = TimelineSessionState {
            session_id: format!("pid-{pid}"),
            pid,
            workload,
            turns: vec![TimelineTurn {
                prompt,
                assistant_stream: String::new(),
                running: true,
            }],
            error: None,
            system_events: Vec::new(),
        };
        self.sessions.insert(pid, state);
    }

    pub fn append_user_turn(&mut self, pid: u64, prompt: String) {
        let Some(session) = self.sessions.get_mut(&pid) else {
            return;
        };
        if let Some(current_turn) = session.turns.last_mut() {
            current_turn.running = false;
        }
        session.error = None;
        session.turns.push(TimelineTurn {
            prompt,
            assistant_stream: String::new(),
            running: true,
        });
    }

    pub fn resume_last_turn(&mut self, pid: u64) {
        let Some(session) = self.sessions.get_mut(&pid) else {
            return;
        };
        if let Some(turn) = session.turns.last_mut() {
            turn.running = true;
        }
        session.error = None;
    }

    pub fn append_assistant_chunk(&mut self, pid: u64, text: &str) {
        if text.is_empty() {
            return;
        }
        let Some(session) = self.sessions.get_mut(&pid) else {
            return;
        };
        let Some(turn) = session.turns.last_mut() else {
            return;
        };
        turn.assistant_stream.push_str(text);
    }

    pub fn finish_session_with_reason(
        &mut self,
        pid: u64,
        marker: Option<ProcessFinishedMarker>,
        reason: Option<&str>,
    ) {
        let Some(session) = self.sessions.get_mut(&pid) else {
            return;
        };
        if let Some(turn) = session.turns.last_mut() {
            turn.running = false;
        }
        if let Some(marker) = marker {
            session.system_events.push((
                format!(
                    "Process finished: pid={} tokens_generated={} elapsed_secs={:.3}",
                    marker.pid, marker.tokens_generated, marker.elapsed_secs
                ),
                "complete".to_string(),
            ));
        } else if let Some(reason) = reason {
            session
                .system_events
                .push((reason.to_string(), "complete".to_string()));
        }
    }

    pub fn set_error(&mut self, pid: u64, error: String) {
        let Some(session) = self.sessions.get_mut(&pid) else {
            return;
        };
        if let Some(turn) = session.turns.last_mut() {
            turn.running = false;
        }
        session.error = Some(error.clone());
        session.system_events.push((error, "error".to_string()));
    }

    pub fn snapshot(&self, pid: u64) -> Option<TimelineSnapshot> {
        self.sessions.get(&pid).map(|session| TimelineSnapshot {
            session_id: session.session_id.clone(),
            pid: session.pid,
            running: session.turns.last().is_some_and(|turn| turn.running),
            workload: session.workload.clone(),
            source: "live_exec".to_string(),
            fallback_notice: None,
            error: session.error.clone(),
            items: build_live_timeline_items(session),
        })
    }

    fn insert_persisted_session(&mut self, session: TimelineSessionState) {
        self.sessions.insert(session.pid, session);
    }
}

fn timeline_sessions_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("timeline_sessions")
}

fn timeline_session_path(workspace_root: &Path, pid: u64) -> PathBuf {
    timeline_sessions_dir(workspace_root).join(format!("pid-{pid}.json"))
}

pub fn persist_session_snapshot(
    workspace_root: &Path,
    store: &TimelineStore,
    pid: u64,
) -> Result<(), String> {
    let Some(session) = store.sessions.get(&pid).cloned() else {
        return Ok(());
    };

    fs::create_dir_all(timeline_sessions_dir(workspace_root)).map_err(|err| err.to_string())?;
    let payload = serde_json::to_vec_pretty(&session).map_err(|err| err.to_string())?;
    fs::write(timeline_session_path(workspace_root, pid), payload).map_err(|err| err.to_string())
}

fn load_persisted_session(
    workspace_root: &Path,
    pid: u64,
) -> Result<Option<TimelineSessionState>, String> {
    let path = timeline_session_path(workspace_root, pid);
    match fs::read(path) {
        Ok(payload) => serde_json::from_slice(&payload)
            .map(Some)
            .map_err(|err| err.to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

fn sync_persisted_session_with_workspace_snapshot(
    session: &mut TimelineSessionState,
    snapshot: &WorkspaceSnapshot,
) {
    session.workload = snapshot.workload.clone();
    if let Some(turn) = session.turns.last_mut() {
        turn.running = matches!(
            snapshot.state.as_str(),
            "Running" | "WaitingForSyscall" | "InFlight"
        );
    }
}

pub fn hydrate_session_from_disk(
    timeline_store: &Arc<Mutex<TimelineStore>>,
    workspace_root: &Path,
    pid: u64,
    snapshot: Option<&WorkspaceSnapshot>,
) -> Result<bool, String> {
    let Some(mut session) = load_persisted_session(workspace_root, pid)? else {
        return Ok(false);
    };

    if let Some(snapshot) = snapshot {
        sync_persisted_session_with_workspace_snapshot(&mut session, snapshot);
    }

    let mut store = timeline_store
        .lock()
        .map_err(|_| "Timeline store lock poisoned".to_string())?;
    store.insert_persisted_session(session);
    Ok(true)
}

pub fn synthesize_fallback_timeline(snapshot: WorkspaceSnapshot) -> TimelineSnapshot {
    let running = matches!(
        snapshot.state.as_str(),
        "Running" | "WaitingForSyscall" | "InFlight"
    );
    let mut items = Vec::new();
    items.push(TimelineItem {
        id: format!("{}-fallback-1", snapshot.session_id),
        kind: TimelineItemKind::SystemEvent,
        text: "Timeline in fallback mode: this PID was not started by the Tauri bridge, so no live EXEC stream is attached.".to_string(),
        status: "degraded".to_string(),
    });
    items.push(TimelineItem {
        id: format!("{}-fallback-2", snapshot.session_id),
        kind: TimelineItemKind::AssistantMessage,
        text: format!(
            "Stato runtime corrente: workload={} state={} elapsed={:.0}s tokens_generated={} syscalls_used={}",
            snapshot.workload,
            snapshot.state,
            snapshot.elapsed_secs.max(0.0),
            snapshot.tokens_generated,
            snapshot.syscalls_used
        ),
        status: if running {
            "degraded-live".to_string()
        } else {
            "complete".to_string()
        },
    });

    for (index, event) in snapshot.audit_events.iter().enumerate() {
        items.push(TimelineItem {
            id: format!("{}-fallback-audit-{}", snapshot.session_id, index + 1),
            kind: TimelineItemKind::SystemEvent,
            text: format!("{}: {}", event.title, event.detail),
            status: "degraded".to_string(),
        });
    }

    TimelineSnapshot {
        session_id: snapshot.session_id,
        pid: snapshot.pid,
        running,
        workload: snapshot.workload,
        source: "status_fallback".to_string(),
        fallback_notice: Some(
            "Oggi il protocollo/kernel non espone un opcode o una capability per attach a uno stream EXEC gia' esistente. In futuro questo fallback puo' essere sostituito da una capability dedicata di stream attach/replay.".to_string(),
        ),
        error: None,
        items,
    }
}

pub fn augment_timeline_with_tool_results(
    mut timeline: TimelineSnapshot,
    audit_entries: &[AuditLogEntry],
) -> TimelineSnapshot {
    if audit_entries.is_empty() {
        return timeline;
    }

    let mut augmented = Vec::new();
    let mut tool_results = audit_entries.iter();
    let mut trailing = Vec::new();

    for item in timeline.items {
        let is_tool_call = matches!(item.kind, TimelineItemKind::ToolCall);
        augmented.push(item);
        if is_tool_call {
            if let Some(entry) = tool_results.next() {
                augmented.push(tool_result_item(
                    &timeline.session_id,
                    augmented.len() + 1,
                    entry,
                ));
            }
        }
    }

    for entry in tool_results {
        trailing.push(tool_result_item(
            &timeline.session_id,
            augmented.len() + trailing.len() + 1,
            entry,
        ));
    }

    augmented.extend(trailing);
    timeline.items = augmented;
    timeline
}

pub fn start_exec_session(
    addr: String,
    workspace_root: PathBuf,
    prompt: String,
    workload: String,
    timeline_store: Arc<Mutex<TimelineStore>>,
) -> Result<StartSessionResult, String> {
    let mut stream = TcpStream::connect(&addr).map_err(|err| err.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| err.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| err.to_string())?;

    authenticate(&mut stream, &workspace_root)?;
    protocol::negotiate_hello(&mut stream).map_err(|err| err.to_string())?;

    let outbound_prompt = if workload.trim().is_empty() || workload == "auto" {
        prompt.clone()
    } else {
        format!("capability={workload}; {prompt}")
    };

    protocol::send_command(&mut stream, OpCode::Exec, "1", outbound_prompt.as_bytes())
        .map_err(|err| err.to_string())?;
    let started_frame = protocol::read_single_frame(&mut stream, Duration::from_secs(5))
        .map_err(|err| err.to_string())?;
    if started_frame.kind != "+OK" {
        return Err(
            protocol::decode_protocol_error(&started_frame.code, &started_frame.payload)
                .to_string(),
        );
    }

    let started = protocol::decode_protocol_data::<ExecStartPayload>(&started_frame.payload)
        .map_err(|err| err.to_string())?;
    if started.pid == 0 {
        return Err("Kernel returned EXEC start without a PID".to_string());
    }

    let session_id = format!("pid-{}", started.pid);
    {
        let mut store = timeline_store
            .lock()
            .map_err(|_| "Timeline store lock poisoned".to_string())?;
        let started_workload = if started.workload.trim().is_empty() {
            workload.clone()
        } else {
            started.workload.clone()
        };
        store.insert_started_session(started.pid, prompt, started_workload);
        let _ = persist_session_snapshot(&workspace_root, &store, started.pid);
    }

    Ok(StartSessionResult {
        session_id,
        pid: started.pid,
    })
}

fn build_live_timeline_items(session: &TimelineSessionState) -> Vec<TimelineItem> {
    let mut items = Vec::new();
    for (turn_index, turn) in session.turns.iter().enumerate() {
        let turn_id = format!("{}-turn-{}", session.session_id, turn_index + 1);
        items.push(TimelineItem {
            id: format!("{turn_id}-user"),
            kind: TimelineItemKind::UserMessage,
            text: turn.prompt.clone(),
            status: "complete".to_string(),
        });
        items.extend(parse_stream_segments(
            &turn_id,
            &turn.assistant_stream,
            turn.running,
        ));
    }

    for (index, (text, status)) in session.system_events.iter().enumerate() {
        items.push(TimelineItem {
            id: format!("{}-system-{}", session.session_id, index + 1),
            kind: TimelineItemKind::SystemEvent,
            text: text.clone(),
            status: status.clone(),
        });
    }

    items
}

fn parse_stream_segments(item_prefix: &str, stream: &str, running: bool) -> Vec<TimelineItem> {
    let mut items = Vec::new();
    let mut cursor = 0usize;
    let mut item_index = 1usize;

    while cursor < stream.len() {
        let remaining = &stream[cursor..];
        let next_marker = find_next_marker(remaining);

        match next_marker {
            None => {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::AssistantMessage,
                        remaining,
                        if running { "streaming" } else { "complete" },
                );
                break;
            }
            Some((offset, MarkerKind::Think)) => {
                if offset > 0 {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::AssistantMessage,
                        &remaining[..offset],
                        "complete",
                    );
                }

                let think_start = cursor + offset + "<think>".len();
                let think_rest = &stream[think_start..];
                if let Some(end_offset) = think_rest.find("</think>") {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::Thinking,
                        &think_rest[..end_offset],
                        "complete",
                    );
                    cursor = think_start + end_offset + "</think>".len();
                } else {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::Thinking,
                        think_rest,
                        if running { "streaming" } else { "complete" },
                    );
                    break;
                }
            }
            Some((offset, MarkerKind::BracketTool)) => {
                if offset > 0 {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::AssistantMessage,
                        &remaining[..offset],
                        "complete",
                    );
                }

                let tool_start = cursor + offset + 2;
                let tool_rest = &stream[tool_start..];
                if let Some(end_offset) = tool_rest.find("]]") {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::ToolCall,
                        &tool_rest[..end_offset],
                        "complete",
                    );
                    cursor = tool_start + end_offset + 2;
                } else {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::ToolCall,
                        tool_rest,
                        if running { "streaming" } else { "complete" },
                    );
                    break;
                }
            }
            Some((offset, MarkerKind::BareTool)) => {
                if offset > 0 {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::AssistantMessage,
                        &remaining[..offset],
                        "complete",
                    );
                }

                let tool_start = cursor + offset;
                let tool_rest = &stream[tool_start..];
                if let Some(line_end_offset) = tool_rest.find('\n') {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::ToolCall,
                        &tool_rest[..line_end_offset],
                        "complete",
                    );
                    cursor = tool_start + line_end_offset + 1;
                } else {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::ToolCall,
                        tool_rest,
                        if running { "streaming" } else { "complete" },
                    );
                    break;
                }
            }
        }
    }

    if items.is_empty() && running {
        items.push(TimelineItem {
            id: format!("{item_prefix}-assistant-waiting"),
            kind: TimelineItemKind::AssistantMessage,
            text: String::new(),
            status: "streaming".to_string(),
        });
    }

    items
}

#[derive(Clone, Copy)]
enum MarkerKind {
    Think,
    BracketTool,
    BareTool,
}

fn find_next_marker(stream: &str) -> Option<(usize, MarkerKind)> {
    let mut in_fenced_block = false;
    let mut absolute_offset = 0usize;

    for line in stream.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let leading_ws = line.len() - trimmed.len();
        let marker_offset = absolute_offset + leading_ws;

        if !in_fenced_block {
            if trimmed.starts_with("<think>") {
                return Some((marker_offset, MarkerKind::Think));
            }

            if trimmed.starts_with("[[") && valid_bracket_tool_marker(stream, marker_offset) {
                return Some((marker_offset, MarkerKind::BracketTool));
            }

            if valid_bare_tool_marker(stream, marker_offset) {
                return Some((marker_offset, MarkerKind::BareTool));
            }
        }

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fenced_block = !in_fenced_block;
        }

        absolute_offset += line.len();
    }

    None
}

fn valid_bracket_tool_marker(stream: &str, marker_offset: usize) -> bool {
    let rest = &stream[marker_offset + 2..];
    match rest.find("]]") {
        Some(end_offset) => looks_like_syscall_invocation(&rest[..end_offset]),
        None => false,
    }
}

fn valid_bare_tool_marker(stream: &str, marker_offset: usize) -> bool {
    let line_end = stream[marker_offset..]
        .find('\n')
        .map(|offset| marker_offset + offset)
        .unwrap_or(stream.len());
    looks_like_syscall_invocation(stream[marker_offset..line_end].trim())
}

fn looks_like_syscall_invocation(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }

    for prefix in [
        "TOOL:",
        "SEND:",
        "SPAWN:",
        "PYTHON:",
        "WRITE_FILE:",
        "READ_FILE:",
        "CALC:",
    ] {
        if trimmed.starts_with(prefix) {
            return true;
        }
    }

    trimmed == "LS" || trimmed.starts_with("LS ")
}

fn push_timeline_text_item(
    items: &mut Vec<TimelineItem>,
    item_prefix: &str,
    item_index: &mut usize,
    kind: TimelineItemKind,
    text: &str,
    status: &str,
) {
    let normalized = text.trim();
    if normalized.is_empty() {
        return;
    }

    items.push(TimelineItem {
        id: format!("{item_prefix}-segment-{}", *item_index),
        kind,
        text: normalized.to_string(),
        status: status.to_string(),
    });
    *item_index += 1;
}

fn tool_result_item(session_id: &str, item_index: usize, entry: &AuditLogEntry) -> TimelineItem {
    let normalized_command = entry
        .command
        .trim()
        .trim_start_matches("[[")
        .trim_end_matches("]]")
        .to_string();
    let status = if entry.success { "success" } else { "error" };
    let text = format!(
        "Command: {}\n\n{}\n\nduration_ms={} kill={}",
        normalized_command, entry.detail, entry.duration_ms, entry.should_kill
    );
    TimelineItem {
        id: format!("{}-tool-result-{}", session_id, item_index),
        kind: TimelineItemKind::ToolResult,
        text,
        status: status.to_string(),
    }
}

fn authenticate(stream: &mut TcpStream, workspace_root: &Path) -> Result<(), String> {
    let token = load_token(workspace_root)?;
    if token.is_empty() {
        return Ok(());
    }

    protocol::send_command(stream, OpCode::Auth, "1", token.as_bytes())
        .map_err(|err| err.to_string())?;
    let response = protocol::read_single_frame(stream, Duration::from_secs(5))
        .map_err(|err| err.to_string())?;
    if response.kind != "+OK" {
        return Err(protocol::decode_protocol_error(&response.code, &response.payload).to_string());
    }
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

#[cfg(test)]
mod tests {
    use super::{parse_stream_segments, TimelineItemKind};

    #[test]
    fn parse_stream_segments_splits_thinking_tool_and_answer() {
        let stream = "<think>step 1\nstep 2</think>\n[[PYTHON: print(2 + 2)]]\nFinal answer";
        let items = parse_stream_segments("pid-1", stream, false);
        assert_eq!(items.len(), 3);
        assert!(matches!(items[0].kind, TimelineItemKind::Thinking));
        assert!(items[0].text.contains("step 1"));
        assert!(matches!(items[1].kind, TimelineItemKind::ToolCall));
        assert!(items[1].text.contains("PYTHON"));
        assert!(matches!(items[2].kind, TimelineItemKind::AssistantMessage));
        assert_eq!(items[2].text, "Final answer");
    }

    #[test]
    fn parse_stream_segments_keeps_open_thinking_streaming() {
        let stream = "Prelude\n<think>reasoning in progress";
        let items = parse_stream_segments("pid-2", stream, true);
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0].kind, TimelineItemKind::AssistantMessage));
        assert_eq!(items[0].status, "complete");
        assert!(matches!(items[1].kind, TimelineItemKind::Thinking));
        assert_eq!(items[1].status, "streaming");
    }

    #[test]
    fn parse_stream_segments_supports_bare_tool_lines() {
        let stream = "Prelude\nTOOL:python {\"code\":\"print(1)\"}\nFinal answer";
        let items = parse_stream_segments("pid-3", stream, false);
        assert_eq!(items.len(), 3);
        assert!(matches!(items[0].kind, TimelineItemKind::AssistantMessage));
        assert!(matches!(items[1].kind, TimelineItemKind::ToolCall));
        assert_eq!(items[1].text, "TOOL:python {\"code\":\"print(1)\"}");
        assert!(matches!(items[2].kind, TimelineItemKind::AssistantMessage));
    }

    #[test]
    fn parse_stream_segments_ignores_inline_marker_like_text() {
        let stream = "Markdown keeps [[NOTE: do not parse]] inline.";
        let items = parse_stream_segments("pid-4", stream, false);
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0].kind, TimelineItemKind::AssistantMessage));
        assert_eq!(items[0].text, stream);
    }

    #[test]
    fn parse_stream_segments_ignores_markers_inside_code_fences() {
        let stream = "```md\nTOOL:python {\"code\":\"print(1)\"}\n[[PYTHON: print(2)]]\n<think>still code</think>\n```\nFinal answer";
        let items = parse_stream_segments("pid-5", stream, false);
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0].kind, TimelineItemKind::AssistantMessage));
        assert_eq!(items[0].text, stream);
    }

    #[test]
    fn parse_stream_segments_ignores_non_syscall_bracket_lines() {
        let stream = "[[NOTE: not a syscall]]\nFinal answer";
        let items = parse_stream_segments("pid-6", stream, false);
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0].kind, TimelineItemKind::AssistantMessage));
        assert_eq!(items[0].text, stream);
    }
}
