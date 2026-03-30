use std::collections::BTreeMap;
use std::path::Path;

use crate::kernel::live_timeline::{parse_stream_segments, TimelineSeedSession, TimelineSeedTurn};
use crate::models::kernel::{TimelineItem, TimelineItemKind, TimelineSnapshot};

use super::db::{
    load_messages, load_session_identity, load_turns, open_connection, StoredMessage, StoredTurn,
};

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
        let mut assistant_stream = String::new();
        let mut system_messages = Vec::new();
        let running = matches!(turn.status.as_str(), "running" | "awaiting_turn_decision");

        if let Some(turn_messages) = grouped.get(&turn.turn_id) {
            let mut ordered = turn_messages.clone();
            ordered.sort_by_key(|message| message.ordinal);

            for message in ordered {
                match message.role.as_str() {
                    "user" if prompt.is_empty() => {
                        prompt = message.content.clone();
                    }
                    "assistant" => {
                        assistant_stream.push_str(&message.content);
                    }
                    "system" => {
                        system_messages.push((
                            message.content.clone(),
                            if message.kind == "error" {
                                "error".to_string()
                            } else {
                                "complete".to_string()
                            },
                        ));
                    }
                    _ => {}
                }
            }
        }

        if !prompt.is_empty() {
            items.push(TimelineItem {
                id: format!("{turn_id}-user"),
                kind: TimelineItemKind::UserMessage,
                text: prompt,
                status: "complete".to_string(),
            });
        }
        items.extend(parse_stream_segments(&turn_id, &assistant_stream, running));

        for (text, status) in system_messages {
            system_index += 1;
            items.push(TimelineItem {
                id: format!("{}-system-{}", session_id, system_index),
                kind: TimelineItemKind::SystemEvent,
                text,
                status,
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
        let mut assistant_stream = String::new();
        let running = matches!(turn.status.as_str(), "running" | "awaiting_turn_decision");

        if let Some(turn_messages) = grouped.remove(&turn.turn_id) {
            let mut ordered = turn_messages;
            ordered.sort_by_key(|message| message.ordinal);

            for message in ordered {
                match message.role.as_str() {
                    "user" if prompt.is_empty() => {
                        prompt = message.content;
                    }
                    "assistant" => {
                        assistant_stream.push_str(&message.content);
                    }
                    "system" => {
                        system_events.push((
                            message.content,
                            if message.kind == "error" {
                                "error".to_string()
                            } else {
                                "complete".to_string()
                            },
                        ));
                    }
                    _ => {}
                }
            }
        }

        if !prompt.is_empty() || !assistant_stream.is_empty() {
            seeded_turns.push(TimelineSeedTurn {
                prompt,
                assistant_stream,
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
