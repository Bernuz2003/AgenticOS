use agentic_control_models::{InvocationEvent, InvocationKind, InvocationStatus};

use crate::models::kernel::{TimelineItem, TimelineItemKind, TimelineSnapshot, WorkspaceSnapshot};

use super::store::{TimelineSessionState, TimelineStore, TimelineTurnMessage};

impl TimelineStore {
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
        "Running" | "WaitingForSyscall" | "InFlight" | "AwaitingRemoteResponse"
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

fn build_live_timeline_items(session: &TimelineSessionState) -> Vec<TimelineItem> {
    let mut items = Vec::new();
    for (turn_index, turn) in session.turns.iter().enumerate() {
        let turn_id = format!("{}-turn-{}", session.session_id, turn_index + 1);
        let mut rendered_turn_messages = false;
        items.push(TimelineItem {
            id: format!("{turn_id}-user"),
            kind: TimelineItemKind::UserMessage,
            text: turn.prompt.clone(),
            status: "complete".to_string(),
        });
        let last_message_index = turn.messages.len().saturating_sub(1);
        for (message_index, message) in turn.messages.iter().enumerate() {
            match message {
                TimelineTurnMessage::Assistant { text } => {
                    if !is_renderable_timeline_text(text) {
                        continue;
                    }
                    rendered_turn_messages = true;
                    items.push(TimelineItem {
                        id: format!("{turn_id}-assistant-{}", message_index + 1),
                        kind: TimelineItemKind::AssistantMessage,
                        text: text.clone(),
                        status: if turn.running && message_index == last_message_index {
                            "streaming".to_string()
                        } else {
                            "complete".to_string()
                        },
                    });
                }
                TimelineTurnMessage::Thinking { text } => {
                    if !is_renderable_timeline_text(text) {
                        continue;
                    }
                    rendered_turn_messages = true;
                    items.push(TimelineItem {
                        id: format!("{turn_id}-thinking-{}", message_index + 1),
                        kind: TimelineItemKind::Thinking,
                        text: text.clone(),
                        status: if turn.running && message_index == last_message_index {
                            "streaming".to_string()
                        } else {
                            "complete".to_string()
                        },
                    });
                }
                TimelineTurnMessage::Invocation { invocation } => {
                    rendered_turn_messages = true;
                    items.push(TimelineItem {
                        id: format!("{turn_id}-invocation-{}", invocation.invocation_id),
                        kind: timeline_item_kind_for_invocation(invocation),
                        text: invocation.command.clone(),
                        status: timeline_item_status_for_invocation(invocation).to_string(),
                    });
                }
            }
        }
        if !rendered_turn_messages && turn.running {
            items.push(TimelineItem {
                id: format!("{turn_id}-assistant-waiting"),
                kind: TimelineItemKind::AssistantMessage,
                text: String::new(),
                status: "streaming".to_string(),
            });
        }
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

fn is_renderable_timeline_text(text: &str) -> bool {
    !text.trim().is_empty()
}

fn timeline_item_kind_for_invocation(invocation: &InvocationEvent) -> TimelineItemKind {
    match invocation.kind {
        InvocationKind::Action => TimelineItemKind::ActionCall,
        InvocationKind::Tool => TimelineItemKind::ToolCall,
    }
}

fn timeline_item_status_for_invocation(invocation: &InvocationEvent) -> &'static str {
    match invocation.status {
        InvocationStatus::Dispatched => "dispatching",
        InvocationStatus::Completed => "complete",
        InvocationStatus::Failed | InvocationStatus::Killed => "error",
    }
}
