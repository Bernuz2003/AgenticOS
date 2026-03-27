use std::collections::HashMap;
use std::fs;
use std::net::TcpStream;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agentic_control_models::ExecStartPayload;
use agentic_protocol::OpCode;

use crate::kernel::auth::kernel_token_path;
use crate::kernel::client::transport;
use crate::models::kernel::StartSessionResult;

#[derive(Debug, Default)]
pub struct TimelineStore {
    pub(super) sessions: HashMap<u64, TimelineSessionState>,
}

#[derive(Debug, Clone)]
pub(super) struct TimelineTurn {
    pub(super) prompt: String,
    pub(super) assistant_stream: String,
    pub(super) running: bool,
}

#[derive(Debug, Clone)]
pub struct TimelineSeedTurn {
    pub prompt: String,
    pub assistant_stream: String,
    pub running: bool,
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
                    assistant_stream: turn.assistant_stream,
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
    transport::negotiate_hello(&mut stream).map_err(|err| err.to_string())?;

    let outbound_prompt = if workload.trim().is_empty() || workload == "auto" {
        prompt.clone()
    } else {
        format!("capability={workload}; {prompt}")
    };

    transport::send_command(&mut stream, OpCode::Exec, "1", outbound_prompt.as_bytes())
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
        return Err(transport::decode_protocol_error(&response.code, &response.payload).to_string());
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
    use super::{TimelineSeedSession, TimelineSeedTurn, TimelineStore};
    use crate::kernel::live_timeline::parse_stream_segments;
    use crate::models::kernel::TimelineItemKind;

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
        assert_eq!(items[1].text, "ACTION:send {\"pid\":1,\"message\":\"hi\"}");
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
        assert!(matches!(
            timeline.items[0].kind,
            TimelineItemKind::UserMessage
        ));
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

    #[test]
    fn seeded_session_can_rebind_to_new_pid_without_losing_history() {
        let mut store = TimelineStore::default();
        store.insert_seeded_session(TimelineSeedSession {
            session_id: "sess-history".to_string(),
            pid: 21,
            workload: "general".to_string(),
            turns: vec![TimelineSeedTurn {
                prompt: "persisted prompt".to_string(),
                assistant_stream: "persisted answer".to_string(),
                running: false,
            }],
            error: None,
            system_events: Vec::new(),
        });

        store.rebind_session_pid("sess-history", 84, "general".to_string());
        store.append_user_turn(84, "new input".to_string());

        assert!(store.snapshot(21).is_none());

        let timeline = store
            .snapshot(84)
            .expect("rebound session should be addressable by the new pid");
        assert_eq!(timeline.session_id, "sess-history");
        assert_eq!(timeline.items.len(), 4);
        assert_eq!(timeline.items[0].text, "persisted prompt");
        assert_eq!(timeline.items[1].text, "persisted answer");
        assert_eq!(timeline.items[2].text, "new input");
        assert!(matches!(
            timeline.items[3].kind,
            TimelineItemKind::AssistantMessage
        ));
        assert_eq!(timeline.items[3].status, "streaming");
        assert!(timeline.running);
    }
}
