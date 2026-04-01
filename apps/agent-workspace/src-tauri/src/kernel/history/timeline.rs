use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use agentic_control_models::{InvocationEvent, InvocationKind, InvocationStatus};

use crate::kernel::live_timeline::{TimelineSeedMessage, TimelineSeedSession, TimelineSeedTurn};
use crate::models::kernel::{TimelineItem, TimelineItemKind, TimelineSnapshot};

use super::db::{
    load_messages, load_session_identity, load_turns, open_connection, StoredMessage, StoredTurn,
};

#[derive(Debug)]
enum ProjectedTurnMessage {
    AssistantMessage(String),
    Thinking(String),
    Invocation(InvocationEvent),
    SystemEvent { text: String, status: String },
}

pub fn load_timeline_snapshot(
    workspace_root: &Path,
    session_id: &str,
    pid_hint: Option<u64>,
) -> Result<Option<TimelineSnapshot>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(None);
    };
    let Some(identity) = load_session_identity(&connection, session_id)? else {
        return Ok(None);
    };
    let turns = load_turns(&connection, &identity.session_id)?;
    let messages = load_messages(&connection, &identity.session_id)?;
    let running = turns
        .last()
        .is_some_and(|turn| matches!(turn.status.as_str(), "running" | "awaiting_turn_decision"));
    let pid = identity
        .active_pid
        .or(identity.last_pid)
        .or_else(|| turns.last().map(|turn| turn.pid))
        .or(pid_hint)
        .unwrap_or(0);
    let workload = turns
        .last()
        .map(|turn| turn.workload.clone())
        .unwrap_or(identity.workload);
    let items = build_timeline_items(&identity.session_id, &turns, &messages);
    let error = messages
        .iter()
        .rev()
        .find(|message| message.kind == "error")
        .map(|message| message.content.clone());

    Ok(Some(TimelineSnapshot {
        session_id: identity.session_id,
        pid,
        running,
        workload,
        source: "sqlite_history".to_string(),
        fallback_notice: None,
        error,
        items,
    }))
}

pub fn load_timeline_seed(
    workspace_root: &Path,
    session_id: &str,
    pid_hint: Option<u64>,
) -> Result<Option<TimelineSeedSession>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(None);
    };
    let Some(identity) = load_session_identity(&connection, session_id)? else {
        return Ok(None);
    };
    let turns = load_turns(&connection, &identity.session_id)?;
    let messages = load_messages(&connection, &identity.session_id)?;
    let pid = identity
        .active_pid
        .or(identity.last_pid)
        .or_else(|| turns.last().map(|turn| turn.pid))
        .or(pid_hint)
        .unwrap_or(0);
    let workload = turns
        .last()
        .map(|turn| turn.workload.clone())
        .unwrap_or(identity.workload);
    let error = messages
        .iter()
        .rev()
        .find(|message| message.kind == "error")
        .map(|message| message.content.clone());

    Ok(Some(build_timeline_seed(
        identity.session_id,
        pid,
        workload,
        turns,
        messages,
        error,
    )))
}

fn build_timeline_items(
    session_id: &str,
    turns: &[StoredTurn],
    messages: &[StoredMessage],
) -> Vec<TimelineItem> {
    let mut grouped = BTreeMap::<i64, Vec<&StoredMessage>>::new();
    for message in messages {
        grouped.entry(message.turn_id).or_default().push(message);
    }

    let mut items = Vec::new();
    let mut system_index = 0usize;
    for turn in turns {
        let turn_id = format!("{}-turn-{}", session_id, turn.turn_index);
        let mut prompt = String::new();
        let mut rendered_turn_messages = false;
        let running = matches!(turn.status.as_str(), "running" | "awaiting_turn_decision");
        let mut projected_turn_messages = Vec::new();

        if let Some(turn_messages) = grouped.get(&turn.turn_id) {
            let mut ordered = turn_messages.clone();
            ordered.sort_by_key(|message| message.ordinal);
            let (projected_prompt, projected_messages) =
                project_turn_messages(ordered.into_iter().map(clone_stored_message));
            prompt = projected_prompt;
            projected_turn_messages = projected_messages;
        }

        if !prompt.is_empty() {
            items.push(TimelineItem {
                id: format!("{turn_id}-user"),
                kind: TimelineItemKind::UserMessage,
                text: prompt,
                status: "complete".to_string(),
            });
        }

        let last_message_index = projected_turn_messages.len().saturating_sub(1);
        for (message_index, message) in projected_turn_messages.into_iter().enumerate() {
            match message {
                ProjectedTurnMessage::AssistantMessage(text) => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    rendered_turn_messages = true;
                    items.push(TimelineItem {
                        id: format!("{turn_id}-assistant-{}", message_index + 1),
                        kind: TimelineItemKind::AssistantMessage,
                        text,
                        status: if running && message_index == last_message_index {
                            "streaming".to_string()
                        } else {
                            "complete".to_string()
                        },
                    });
                }
                ProjectedTurnMessage::Thinking(text) => {
                    rendered_turn_messages = true;
                    items.push(TimelineItem {
                        id: format!("{turn_id}-thinking-{}", message_index + 1),
                        kind: TimelineItemKind::Thinking,
                        text,
                        status: if running && message_index == last_message_index {
                            "streaming".to_string()
                        } else {
                            "complete".to_string()
                        },
                    });
                }
                ProjectedTurnMessage::Invocation(invocation) => {
                    rendered_turn_messages = true;
                    items.push(TimelineItem {
                        id: format!("{turn_id}-invocation-{}", invocation.invocation_id),
                        kind: timeline_item_kind_for_invocation(&invocation),
                        text: invocation.command.clone(),
                        status: timeline_item_status_for_invocation(&invocation).to_string(),
                    });
                }
                ProjectedTurnMessage::SystemEvent { text, status } => {
                    system_index += 1;
                    items.push(TimelineItem {
                        id: format!("{}-system-{}", session_id, system_index),
                        kind: TimelineItemKind::SystemEvent,
                        text,
                        status,
                    });
                }
            }
        }

        if !rendered_turn_messages && running {
            items.push(TimelineItem {
                id: format!("{turn_id}-assistant-waiting"),
                kind: TimelineItemKind::AssistantMessage,
                text: String::new(),
                status: "streaming".to_string(),
            });
        }
    }

    items
}

fn build_timeline_seed(
    session_id: String,
    pid: u64,
    workload: String,
    turns: Vec<StoredTurn>,
    messages: Vec<StoredMessage>,
    error: Option<String>,
) -> TimelineSeedSession {
    let mut grouped = BTreeMap::<i64, Vec<StoredMessage>>::new();
    for message in messages {
        grouped.entry(message.turn_id).or_default().push(message);
    }

    let mut seeded_turns = Vec::new();
    let mut system_events = Vec::new();

    for turn in turns {
        let mut prompt = String::new();
        let mut turn_messages = Vec::new();
        let running = matches!(turn.status.as_str(), "running" | "awaiting_turn_decision");

        if let Some(turn_messages_raw) = grouped.remove(&turn.turn_id) {
            let mut ordered = turn_messages_raw;
            ordered.sort_by_key(|message| message.ordinal);

            let (projected_prompt, projected_messages) = project_turn_messages(ordered);
            prompt = projected_prompt;

            for message in projected_messages {
                match message {
                    ProjectedTurnMessage::AssistantMessage(text) => {
                        turn_messages.push(TimelineSeedMessage::Assistant(text));
                    }
                    ProjectedTurnMessage::Thinking(text) => {
                        turn_messages.push(TimelineSeedMessage::Thinking(text));
                    }
                    ProjectedTurnMessage::Invocation(invocation) => {
                        turn_messages.push(TimelineSeedMessage::Invocation(invocation));
                    }
                    ProjectedTurnMessage::SystemEvent { text, status } => {
                        system_events.push((text, status));
                    }
                }
            }
        }

        if !prompt.is_empty() {
            seeded_turns.push(TimelineSeedTurn {
                prompt,
                messages: turn_messages,
                running,
            });
        }
    }

    TimelineSeedSession {
        session_id,
        pid,
        workload,
        turns: seeded_turns,
        error,
        system_events,
    }
}

fn project_turn_messages<I>(messages: I) -> (String, Vec<ProjectedTurnMessage>)
where
    I: IntoIterator<Item = StoredMessage>,
{
    let mut prompt = String::new();
    let mut projected = Vec::new();
    let mut invocation_positions = HashMap::<String, usize>::new();

    for message in messages {
        match message.role.as_str() {
            "user" if prompt.is_empty() => {
                prompt = message.content;
            }
            "assistant" if message.kind == "thinking" => {
                projected.push(ProjectedTurnMessage::Thinking(message.content));
            }
            "assistant" => {
                projected.push(ProjectedTurnMessage::AssistantMessage(message.content));
            }
            "system" if message.kind == "invocation" => {
                if let Some(invocation) = parse_invocation_message(&message.content) {
                    if let Some(position) =
                        invocation_positions.get(&invocation.invocation_id).copied()
                    {
                        projected[position] = ProjectedTurnMessage::Invocation(invocation);
                    } else {
                        invocation_positions
                            .insert(invocation.invocation_id.clone(), projected.len());
                        projected.push(ProjectedTurnMessage::Invocation(invocation));
                    }
                } else {
                    projected.push(ProjectedTurnMessage::SystemEvent {
                        text: message.content,
                        status: "error".to_string(),
                    });
                }
            }
            "system" => {
                projected.push(ProjectedTurnMessage::SystemEvent {
                    text: message.content,
                    status: if message.kind == "error" {
                        "error".to_string()
                    } else {
                        "complete".to_string()
                    },
                });
            }
            _ => {}
        }
    }

    (prompt, projected)
}

fn clone_stored_message(message: &StoredMessage) -> StoredMessage {
    StoredMessage {
        turn_id: message.turn_id,
        ordinal: message.ordinal,
        role: message.role.clone(),
        kind: message.kind.clone(),
        content: message.content.clone(),
    }
}

fn parse_invocation_message(content: &str) -> Option<InvocationEvent> {
    serde_json::from_str(content).ok()
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

#[cfg(test)]
mod tests {
    use agentic_control_models::{InvocationKind, InvocationStatus};

    use super::*;

    #[test]
    fn timeline_items_collapse_invocation_lifecycle_to_latest_status() {
        let turns = vec![StoredTurn {
            turn_id: 1,
            turn_index: 1,
            pid: 7,
            workload: "general".to_string(),
            status: "completed".to_string(),
            finish_reason: Some("turn_completed".to_string()),
        }];
        let messages = vec![
            stored_message(1, 1, "user", "prompt", "prompt"),
            stored_message(1, 2, "assistant", "message", "Prima"),
            stored_invocation_message(
                1,
                3,
                "tool-1",
                InvocationStatus::Dispatched,
                r#"TOOL:find_files {"pattern":"*.md"}"#,
            ),
            stored_message(1, 4, "assistant", "message", "Dopo"),
            stored_invocation_message(
                1,
                5,
                "tool-1",
                InvocationStatus::Completed,
                r#"TOOL:find_files {"pattern":"*.md"}"#,
            ),
        ];

        let items = build_timeline_items("sess-1", &turns, &messages);
        assert_eq!(items.len(), 4);
        assert!(matches!(items[1].kind, TimelineItemKind::AssistantMessage));
        assert!(matches!(items[2].kind, TimelineItemKind::ToolCall));
        assert_eq!(items[2].status, "complete");
        assert_eq!(items[2].text, r#"TOOL:find_files {"pattern":"*.md"}"#);
        assert!(matches!(items[3].kind, TimelineItemKind::AssistantMessage));
        assert_eq!(
            items.iter().filter(|item| item.id == items[2].id).count(),
            1
        );
    }

    #[test]
    fn timeline_seed_collapse_keeps_single_invocation_in_original_position() {
        let turns = vec![StoredTurn {
            turn_id: 1,
            turn_index: 1,
            pid: 7,
            workload: "general".to_string(),
            status: "completed".to_string(),
            finish_reason: Some("turn_completed".to_string()),
        }];
        let messages = vec![
            stored_message(1, 1, "user", "prompt", "prompt"),
            stored_message(1, 2, "assistant", "message", "Prima"),
            stored_invocation_message(
                1,
                3,
                "tool-1",
                InvocationStatus::Dispatched,
                r#"TOOL:find_files {"pattern":"*.md"}"#,
            ),
            stored_message(1, 4, "assistant", "message", "Dopo"),
            stored_invocation_message(
                1,
                5,
                "tool-1",
                InvocationStatus::Completed,
                r#"TOOL:find_files {"pattern":"*.md"}"#,
            ),
        ];

        let seed = build_timeline_seed(
            "sess-1".to_string(),
            7,
            "general".to_string(),
            turns,
            messages,
            None,
        );
        assert_eq!(seed.turns.len(), 1);
        assert_eq!(seed.turns[0].messages.len(), 3);
        match &seed.turns[0].messages[0] {
            TimelineSeedMessage::Assistant(text) => assert_eq!(text, "Prima"),
            other => panic!("unexpected first seed message: {:?}", other),
        }
        match &seed.turns[0].messages[1] {
            TimelineSeedMessage::Invocation(invocation) => {
                assert_eq!(invocation.invocation_id, "tool-1");
                assert_eq!(invocation.status, InvocationStatus::Completed);
            }
            other => panic!("unexpected second seed message: {:?}", other),
        }
        match &seed.turns[0].messages[2] {
            TimelineSeedMessage::Assistant(text) => assert_eq!(text, "Dopo"),
            other => panic!("unexpected third seed message: {:?}", other),
        }
    }

    fn stored_message(
        turn_id: i64,
        ordinal: i64,
        role: &str,
        kind: &str,
        content: &str,
    ) -> StoredMessage {
        StoredMessage {
            turn_id,
            ordinal,
            role: role.to_string(),
            kind: kind.to_string(),
            content: content.to_string(),
        }
    }

    fn stored_invocation_message(
        turn_id: i64,
        ordinal: i64,
        invocation_id: &str,
        status: InvocationStatus,
        command: &str,
    ) -> StoredMessage {
        stored_message(
            turn_id,
            ordinal,
            "system",
            "invocation",
            &serde_json::to_string(&InvocationEvent {
                invocation_id: invocation_id.to_string(),
                kind: InvocationKind::Tool,
                command: command.to_string(),
                status,
            })
            .expect("serialize invocation event"),
        )
    }
}
