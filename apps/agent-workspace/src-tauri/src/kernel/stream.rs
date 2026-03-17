use std::collections::HashMap;
use std::fs;
use std::net::TcpStream;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agentic_control_models::ExecStartPayload;
use agentic_protocol::OpCode;

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

#[derive(Debug, Clone)]
struct TimelineTurn {
    prompt: String,
    assistant_stream: String,
    running: bool,
}

#[derive(Debug, Clone)]
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
    pub fn insert_started_session(
        &mut self,
        pid: u64,
        session_id: String,
        prompt: String,
        workload: String,
    ) {
        // Keep at most one live entry per session_id to avoid accidental aliasing.
        self.sessions
            .retain(|existing_pid, session| *existing_pid == pid || session.session_id != session_id);

        let state = TimelineSessionState {
            session_id,
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

    pub fn evict_session(&mut self, pid: u64) {
        self.sessions.remove(&pid);
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

    pub fn snapshot_for_session_id(&self, session_id: &str) -> Option<TimelineSnapshot> {
        self.sessions
            .values()
            .find(|session| session.session_id == session_id)
            .map(|session| TimelineSnapshot {
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
    let total_tool_calls = timeline
        .items
        .iter()
        .filter(|item| matches!(item.kind, TimelineItemKind::ToolCall))
        .count();

    if audit_entries.is_empty() && total_tool_calls == 0 {
        return timeline;
    }

    let mut augmented = Vec::new();
    let mut tool_results = audit_entries.iter().peekable();
    let mut tool_call_ordinal = 0usize;

    for mut item in timeline.items {
        if matches!(item.kind, TimelineItemKind::ToolCall) {
            tool_call_ordinal += 1;
            let is_last_tool_call = tool_call_ordinal == total_tool_calls;
            let has_audit_result = tool_results.peek().is_some();

            if timeline.running && is_last_tool_call && !has_audit_result {
                item.status = "dispatching".to_string();
            }

            augmented.push(item);
            if let Some(entry) = tool_results.next() {
                augmented.push(tool_result_item(
                    &timeline.session_id,
                    augmented.len() + 1,
                    entry,
                ));
            } else if timeline.running && is_last_tool_call {
                augmented.push(tool_dispatch_item(
                    &timeline.session_id,
                    augmented.len() + 1,
                ));
            }
        } else {
            augmented.push(item);
        }
    }

    // Do not append unmatched audit entries.
    // Audit is currently pid-scoped, so trailing entries may belong to a previous
    // session that used the same PID. Only pair results with visible ToolCall items.
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

    let session_id = started.session_id.clone();
    {
        let mut store = timeline_store
            .lock()
            .map_err(|_| "Timeline store lock poisoned".to_string())?;
        let started_workload = if started.workload.trim().is_empty() {
            workload.clone()
        } else {
            started.workload.clone()
        };
        store.insert_started_session(started.pid, session_id.clone(), prompt, started_workload);
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

pub(super) fn parse_stream_segments(
    item_prefix: &str,
    stream: &str,
    running: bool,
) -> Vec<TimelineItem> {
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
                    let invocation = &tool_rest[..end_offset];
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        timeline_item_kind_for_invocation(invocation),
                        invocation,
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
                if tool_rest.starts_with("TOOL:") || tool_rest.starts_with("ACTION:") {
                    match extract_canonical_invocation_segment(tool_rest) {
                        CanonicalInvocationSegment::Parsed { raw, consumed } => {
                            push_timeline_text_item(
                                &mut items,
                                item_prefix,
                                &mut item_index,
                                timeline_item_kind_for_invocation(raw),
                                raw,
                                "complete",
                            );
                            cursor = tool_start + consumed;
                        }
                        CanonicalInvocationSegment::Incomplete => {
                            push_timeline_text_item(
                                &mut items,
                                item_prefix,
                                &mut item_index,
                                timeline_item_kind_for_invocation(tool_rest),
                                tool_rest,
                                if running { "streaming" } else { "complete" },
                            );
                            break;
                        }
                        CanonicalInvocationSegment::Invalid => {
                            if let Some(line_end_offset) = tool_rest.find('\n') {
                                let invocation = &tool_rest[..line_end_offset];
                                push_timeline_text_item(
                                    &mut items,
                                    item_prefix,
                                    &mut item_index,
                                    timeline_item_kind_for_invocation(invocation),
                                    invocation,
                                    "complete",
                                );
                                cursor = tool_start + line_end_offset + 1;
                            } else {
                                push_timeline_text_item(
                                    &mut items,
                                    item_prefix,
                                    &mut item_index,
                                    timeline_item_kind_for_invocation(tool_rest),
                                    tool_rest,
                                    if running { "streaming" } else { "complete" },
                                );
                                break;
                            }
                        }
                    }
                } else if let Some(line_end_offset) = tool_rest.find('\n') {
                    let invocation = &tool_rest[..line_end_offset];
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        timeline_item_kind_for_invocation(invocation),
                        invocation,
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

// Keep this brace-aware extractor aligned with the kernel logic used for syscall interception.
enum CanonicalInvocationSegment<'a> {
    Parsed { raw: &'a str, consumed: usize },
    Incomplete,
    Invalid,
}

fn extract_canonical_invocation_segment(text: &str) -> CanonicalInvocationSegment<'_> {
    let prefix = if text.starts_with("TOOL:") {
        "TOOL:"
    } else if text.starts_with("ACTION:") {
        "ACTION:"
    } else {
        return CanonicalInvocationSegment::Invalid;
    };

    let Some(rest_with_ws) = text.strip_prefix(prefix) else {
        return CanonicalInvocationSegment::Invalid;
    };
    let rest = rest_with_ws.trim_start();
    let Some(separator_idx) = rest.find(|c: char| c.is_whitespace() || c == '{') else {
        return CanonicalInvocationSegment::Incomplete;
    };

    let name = &rest[..separator_idx];
    if name.is_empty()
        || !name.chars().all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-' | '.')
        })
    {
        return CanonicalInvocationSegment::Invalid;
    }

    let payload_with_ws = &rest[separator_idx..];
    let payload = payload_with_ws.trim_start();
    if payload.is_empty() {
        return CanonicalInvocationSegment::Incomplete;
    }
    if !payload.starts_with('{') {
        return CanonicalInvocationSegment::Invalid;
    }

    let Some(json_end_rel) = first_balanced_json_object_end(payload) else {
        return CanonicalInvocationSegment::Incomplete;
    };

    if serde_json::from_str::<serde_json::Value>(&payload[..json_end_rel]).is_err() {
        return CanonicalInvocationSegment::Invalid;
    }

    let leading_ws_after_prefix = rest_with_ws.len() - rest.len();
    let ws_before_payload = payload_with_ws.len() - payload.len();
    let consumed =
        prefix.len() + leading_ws_after_prefix + separator_idx + ws_before_payload + json_end_rel;

    CanonicalInvocationSegment::Parsed {
        raw: &text[..consumed],
        consumed,
    }
}

fn first_balanced_json_object_end(payload: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in payload.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(idx + ch.len_utf8());
                }
            }
            _ => {}
        }
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
        "ACTION:",
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

fn timeline_item_kind_for_invocation(content: &str) -> TimelineItemKind {
    let trimmed = content.trim();
    if trimmed.starts_with("ACTION:")
        || trimmed.starts_with("SPAWN:")
        || trimmed.starts_with("SEND:")
    {
        TimelineItemKind::ActionCall
    } else {
        TimelineItemKind::ToolCall
    }
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
    let mut metadata = vec![format!(
        "duration_ms={} kill={}",
        entry.duration_ms, entry.should_kill
    )];
    if let Some(tool_name) = entry.tool_name.as_deref() {
        metadata.push(format!("tool={tool_name}"));
    }
    if let Some(caller) = entry.caller.as_deref() {
        metadata.push(format!("caller={caller}"));
    }
    if let Some(transport) = entry.transport.as_deref() {
        metadata.push(format!("transport={transport}"));
    }
    let text = format!(
        "Command: {}\n\n{}\n\n{}",
        normalized_command,
        entry.detail,
        metadata.join(" ")
    );
    TimelineItem {
        id: format!("{}-tool-result-{}", session_id, item_index),
        kind: TimelineItemKind::ToolResult,
        text,
        status: status.to_string(),
    }
}

fn tool_dispatch_item(session_id: &str, item_index: usize) -> TimelineItem {
    TimelineItem {
        id: format!("{}-tool-dispatch-{}", session_id, item_index),
        kind: TimelineItemKind::SystemEvent,
        text: "Tool dispatch in progress: il kernel ha intercettato la syscall e sta aspettando il risultato del worker.".to_string(),
        status: "streaming".to_string(),
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
    use super::{
        augment_timeline_with_tool_results, parse_stream_segments, TimelineItemKind, TimelineStore,
    };
    use crate::kernel::audit::AuditLogEntry;
    use crate::models::kernel::TimelineSnapshot;

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
    fn parse_stream_segments_supports_action_lines() {
        let stream = "Prelude\nACTION:send {\"pid\":42,\"message\":\"hello\"}\nFinal answer";
        let items = parse_stream_segments("pid-action", stream, false);
        assert_eq!(items.len(), 3);
        assert!(matches!(items[1].kind, TimelineItemKind::ActionCall));
        assert_eq!(
            items[1].text,
            "ACTION:send {\"pid\":42,\"message\":\"hello\"}"
        );
    }

    #[test]
    fn parse_stream_segments_splits_canonical_tool_from_inline_suffix() {
        let stream = "TOOL:python {\"code\":\"print(1)\"}La sequenza e' pronta";
        let items = parse_stream_segments("pid-inline", stream, false);
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0].kind, TimelineItemKind::ToolCall));
        assert_eq!(items[0].text, "TOOL:python {\"code\":\"print(1)\"}");
        assert!(matches!(items[1].kind, TimelineItemKind::AssistantMessage));
        assert_eq!(items[1].text, "La sequenza e' pronta");
    }

    #[test]
    fn parse_stream_segments_splits_first_action_from_chained_action() {
        let stream =
            "ACTION:spawn {\"prompt\":\"worker\"}ACTION:send {\"pid\":1,\"message\":\"hi\"}";
        let items = parse_stream_segments("pid-chain", stream, false);
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0].kind, TimelineItemKind::ActionCall));
        assert_eq!(items[0].text, "ACTION:spawn {\"prompt\":\"worker\"}");
        assert!(matches!(items[1].kind, TimelineItemKind::ActionCall));
        assert_eq!(
            items[1].text,
            "ACTION:send {\"pid\":1,\"message\":\"hi\"}"
        );
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

    #[test]
    fn augment_timeline_marks_last_tool_call_as_dispatching_when_audit_is_pending() {
        let timeline = TimelineSnapshot {
            session_id: "pid-7".to_string(),
            pid: 7,
            running: true,
            workload: "code".to_string(),
            source: "live_exec".to_string(),
            fallback_notice: None,
            error: None,
            items: parse_stream_segments(
                "pid-7-turn-1",
                "TOOL:calc {\"expression\":\"1+1\"}\n",
                true,
            ),
        };

        let augmented = augment_timeline_with_tool_results(timeline, &[]);
        assert_eq!(augmented.items.len(), 2);
        assert!(matches!(
            augmented.items[0].kind,
            TimelineItemKind::ToolCall
        ));
        assert_eq!(augmented.items[0].status, "dispatching");
        assert!(matches!(
            augmented.items[1].kind,
            TimelineItemKind::SystemEvent
        ));
        assert_eq!(augmented.items[1].status, "streaming");
    }

    #[test]
    fn augment_timeline_pairs_completed_tool_calls_with_audit_results() {
        let timeline = TimelineSnapshot {
            session_id: "pid-8".to_string(),
            pid: 8,
            running: false,
            workload: "code".to_string(),
            source: "live_exec".to_string(),
            fallback_notice: None,
            error: None,
            items: parse_stream_segments("pid-8-turn-1", "[[CALC: 2 + 2]]\n", false),
        };
        let audit_entries = vec![AuditLogEntry {
            pid: 8,
            success: true,
            should_kill: false,
            duration_ms: 4,
            caller: Some("agent_text".to_string()),
            transport: Some("text".to_string()),
            tool_name: Some("calc".to_string()),
            command: "[[CALC: 2 + 2]]".to_string(),
            detail: "Output:\n4".to_string(),
        }];

        let augmented = augment_timeline_with_tool_results(timeline, &audit_entries);
        assert_eq!(augmented.items.len(), 2);
        assert!(matches!(
            augmented.items[0].kind,
            TimelineItemKind::ToolCall
        ));
        assert!(matches!(
            augmented.items[1].kind,
            TimelineItemKind::ToolResult
        ));
        assert_eq!(augmented.items[1].status, "success");
    }

    #[test]
    fn augment_timeline_ignores_unmatched_audit_entries() {
        let timeline = TimelineSnapshot {
            session_id: "pid-clean".to_string(),
            pid: 77,
            running: true,
            workload: "general".to_string(),
            source: "live_exec".to_string(),
            fallback_notice: None,
            error: None,
            items: vec![crate::models::kernel::TimelineItem {
                id: "pid-clean-turn-1-user".to_string(),
                kind: TimelineItemKind::UserMessage,
                text: "new prompt".to_string(),
                status: "complete".to_string(),
            }],
        };
        let audit_entries = vec![AuditLogEntry {
            pid: 77,
            success: true,
            should_kill: false,
            duration_ms: 3,
            caller: Some("agent_text".to_string()),
            transport: Some("text".to_string()),
            tool_name: Some("python".to_string()),
            command: "TOOL:python {\"code\":\"print(1)\"}".to_string(),
            detail: "Output:\n1".to_string(),
        }];

        let augmented = augment_timeline_with_tool_results(timeline, &audit_entries);
        assert_eq!(augmented.items.len(), 1);
        assert!(matches!(
            augmented.items[0].kind,
            TimelineItemKind::UserMessage
        ));
    }

    #[test]
    fn insert_started_session_resets_live_state_when_pid_is_reused() {
        let mut store = TimelineStore::default();
        store.insert_started_session(
            42,
            "sess-old".to_string(),
            "old prompt".to_string(),
            "general".to_string(),
        );
        store.append_assistant_chunk(42, "old output");
        store.finish_session_with_reason(42, None, Some("completed"));

        store.insert_started_session(
            42,
            "sess-new".to_string(),
            "new prompt".to_string(),
            "general".to_string(),
        );

        let timeline = store
            .snapshot_for_session_id("sess-new")
            .expect("new session timeline should exist");
        assert_eq!(timeline.items.len(), 2);
        assert!(matches!(timeline.items[0].kind, TimelineItemKind::UserMessage));
        assert_eq!(timeline.items[0].text, "new prompt");
        assert!(matches!(
            timeline.items[1].kind,
            TimelineItemKind::AssistantMessage
        ));
        assert_eq!(timeline.items[1].status, "streaming");
        assert!(timeline.running);
    }

    #[test]
    fn evict_session_removes_pid_from_live_store() {
        let mut store = TimelineStore::default();
        store.insert_started_session(
            7,
            "sess-evict".to_string(),
            "prompt".to_string(),
            "general".to_string(),
        );
        assert!(store.snapshot(7).is_some());

        store.evict_session(7);
        assert!(store.snapshot(7).is_none());
        assert!(store.snapshot_for_session_id("sess-evict").is_none());
    }
}
