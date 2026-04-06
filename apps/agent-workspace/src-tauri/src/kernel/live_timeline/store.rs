use agentic_control_models::AssistantSegmentKind;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::net::TcpStream;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agentic_control_models::{ExecStartPayload, InvocationEvent};
use agentic_protocol::OpCode;

use crate::kernel::auth::kernel_token_path;
use crate::kernel::client::transport;
use crate::models::kernel::{SessionPathGrantInput, StartSessionResult};

#[derive(Debug, Default)]
pub struct TimelineStore {
    pub(super) sessions: HashMap<u64, TimelineSessionState>,
}

#[derive(Debug, Clone)]
pub(super) struct TimelineTurn {
    pub(super) prompt: String,
    pub(super) messages: Vec<TimelineTurnMessage>,
    pub(super) running: bool,
}

#[derive(Debug, Clone)]
pub(super) enum TimelineTurnMessage {
    Assistant { text: String },
    Thinking { text: String },
    Invocation { invocation: InvocationEvent },
}

#[derive(Debug, Clone)]
pub struct TimelineSeedTurn {
    pub prompt: String,
    pub messages: Vec<TimelineSeedMessage>,
    pub running: bool,
}

#[derive(Debug, Clone)]
pub enum TimelineSeedMessage {
    Assistant(String),
    Thinking(String),
    Invocation(InvocationEvent),
}

#[derive(Debug, Clone)]
pub struct TimelineSeedSession {
    pub session_id: String,
    pub pid: u64,
    pub workload: String,
    pub turns: Vec<TimelineSeedTurn>,
    pub error: Option<String>,
    pub system_events: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub(super) struct TimelineSessionState {
    pub(super) session_id: String,
    pub(super) pid: u64,
    pub(super) workload: String,
    pub(super) turns: Vec<TimelineTurn>,
    pub(super) error: Option<String>,
    pub(super) system_events: Vec<(String, String)>,
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
        self.sessions.retain(|existing_pid, session| {
            *existing_pid == pid || session.session_id != session_id
        });

        let state = TimelineSessionState {
            session_id,
            pid,
            workload,
            turns: vec![TimelineTurn {
                prompt,
                messages: Vec::new(),
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

    pub fn evict_session_by_id(&mut self, session_id: &str) {
        self.sessions
            .retain(|_, session| session.session_id != session_id);
    }

    pub fn insert_seeded_session(&mut self, seed: TimelineSeedSession) {
        self.sessions.retain(|existing_pid, session| {
            *existing_pid == seed.pid || session.session_id != seed.session_id
        });

        let turns = if seed.turns.is_empty() {
            Vec::new()
        } else {
            seed.turns
                .into_iter()
                .map(|turn| TimelineTurn {
                    prompt: turn.prompt,
                    messages: turn
                        .messages
                        .into_iter()
                        .map(|message| match message {
                            TimelineSeedMessage::Assistant(text) => {
                                TimelineTurnMessage::Assistant { text }
                            }
                            TimelineSeedMessage::Thinking(text) => {
                                TimelineTurnMessage::Thinking { text }
                            }
                            TimelineSeedMessage::Invocation(invocation) => {
                                TimelineTurnMessage::Invocation { invocation }
                            }
                        })
                        .collect(),
                    running: turn.running,
                })
                .collect()
        };

        self.sessions.insert(
            seed.pid,
            TimelineSessionState {
                session_id: seed.session_id,
                pid: seed.pid,
                workload: seed.workload,
                turns,
                error: seed.error,
                system_events: seed.system_events,
            },
        );
    }

    pub fn insert_empty_session(&mut self, pid: u64, session_id: String, workload: String) {
        self.insert_seeded_session(TimelineSeedSession {
            session_id,
            pid,
            workload,
            turns: Vec::new(),
            error: None,
            system_events: Vec::new(),
        });
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
            messages: Vec::new(),
            running: true,
        });
    }

    pub fn last_turn_matches_pending_user_prompt(&self, pid: u64, prompt: &str) -> bool {
        self.sessions
            .get(&pid)
            .and_then(|session| session.turns.last())
            .is_some_and(|turn| turn.running && turn.messages.is_empty() && turn.prompt == prompt)
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

    pub fn append_timeline_segment(&mut self, pid: u64, kind: AssistantSegmentKind, text: &str) {
        if text.is_empty() {
            return;
        }
        let Some(session) = self.sessions.get_mut(&pid) else {
            return;
        };
        let Some(turn) = session.turns.last_mut() else {
            return;
        };
        match turn.messages.last_mut() {
            Some(TimelineTurnMessage::Assistant { text: existing })
                if kind == AssistantSegmentKind::Message =>
            {
                existing.push_str(text)
            }
            Some(TimelineTurnMessage::Thinking { text: existing })
                if kind == AssistantSegmentKind::Thinking =>
            {
                existing.push_str(text)
            }
            _ => turn.messages.push(match kind {
                AssistantSegmentKind::Message => TimelineTurnMessage::Assistant {
                    text: text.to_string(),
                },
                AssistantSegmentKind::Thinking => TimelineTurnMessage::Thinking {
                    text: text.to_string(),
                },
            }),
        }
    }

    pub fn upsert_invocation(&mut self, pid: u64, invocation: InvocationEvent) {
        let Some(session) = self.sessions.get_mut(&pid) else {
            return;
        };
        let Some(turn) = session.turns.last_mut() else {
            return;
        };

        if let Some(existing) = turn.messages.iter_mut().find_map(|message| match message {
            TimelineTurnMessage::Invocation {
                invocation: existing,
            } if existing.invocation_id == invocation.invocation_id => Some(existing),
            _ => None,
        }) {
            *existing = invocation;
            return;
        }

        turn.messages
            .push(TimelineTurnMessage::Invocation { invocation });
    }

    pub(crate) fn finish_session_with_reason(
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

    pub(crate) fn set_error(&mut self, pid: u64, error: String) {
        let Some(session) = self.sessions.get_mut(&pid) else {
            return;
        };
        if let Some(turn) = session.turns.last_mut() {
            turn.running = false;
        }
        session.error = Some(error.clone());
        session.system_events.push((error, "error".to_string()));
    }
}

pub fn start_exec_session(
    addr: String,
    workspace_root: PathBuf,
    prompt: String,
    quota_tokens: Option<u64>,
    quota_syscalls: Option<u64>,
    allowed_tools: Option<Vec<String>>,
    path_grants: Option<Vec<SessionPathGrantInput>>,
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
    transport::negotiate_hello(&mut stream).map_err(|err| err.to_string())?;

    let outbound_payload = serde_json::to_vec(&json!({
        "prompt": prompt.clone(),
        "max_tokens": quota_tokens,
        "max_syscalls": quota_syscalls,
        "allowed_tools": allowed_tools,
        "path_grants": path_grants,
    }))
    .map_err(|err| err.to_string())?;

    transport::send_command(&mut stream, OpCode::Exec, "1", &outbound_payload)
        .map_err(|err| err.to_string())?;
    let started_frame = transport::read_single_frame(&mut stream, Duration::from_secs(5))
        .map_err(|err| err.to_string())?;
    if started_frame.kind != "+OK" {
        return Err(
            transport::decode_protocol_error(&started_frame.code, &started_frame.payload)
                .to_string(),
        );
    }

    let started = transport::decode_protocol_data::<ExecStartPayload>(&started_frame.payload)
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
            "general".to_string()
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

fn authenticate(stream: &mut TcpStream, workspace_root: &Path) -> Result<(), String> {
    let token = load_token(workspace_root)?;
    if token.is_empty() {
        return Ok(());
    }

    transport::send_command(stream, OpCode::Auth, "1", token.as_bytes())
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

fn load_token(workspace_root: &Path) -> Result<String, String> {
    let token_path = kernel_token_path(workspace_root);
    match fs::read_to_string(token_path) {
        Ok(token) => Ok(token.trim().to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err.to_string()),
    }
}
