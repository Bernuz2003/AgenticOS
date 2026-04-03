use agentic_control_models::AssistantSegmentKind;

use super::types::{
    AssistantSemanticEvent, AssistantTransportEvent, AssistantTurnDelta, InFlightAssistantTurn,
    ThinkingSource,
};

fn trailing_partial_syscall_marker_offset(buffer: &str) -> Option<usize> {
    let prefixes = ["ACTION:", "TOOL:"];
    let mut best: Option<usize> = None;

    for prefix in prefixes {
        for candidate_len in 1..prefix.len() {
            if buffer.ends_with(&prefix[..candidate_len]) {
                let start_offset = buffer.len() - candidate_len;
                best = Some(best.map_or(start_offset, |current| current.min(start_offset)));
            }
        }
    }

    best
}

fn trailing_partial_marker_offset(buffer: &str, marker: &str) -> Option<usize> {
    for candidate_len in 1..marker.len() {
        if buffer.ends_with(&marker[..candidate_len]) {
            return Some(buffer.len() - candidate_len);
        }
    }

    None
}

fn find_next_think_marker(stream: &str) -> Option<usize> {
    let mut in_fenced_block = false;
    let mut absolute_offset = 0usize;

    for line in stream.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fenced_block = !in_fenced_block;
            absolute_offset += line.len();
            continue;
        }

        if !in_fenced_block {
            if let Some(offset) = line.find("<think>") {
                return Some(absolute_offset + offset);
            }
        }

        absolute_offset += line.len();
    }

    None
}

fn push_text_event(
    delta: &mut AssistantTurnDelta,
    kind: AssistantSegmentKind,
    text: String,
    source: ThinkingSource,
) {
    if text.is_empty() {
        return;
    }

    match kind {
        AssistantSegmentKind::Message => delta.push(AssistantSemanticEvent::VisibleText(text)),
        AssistantSegmentKind::Thinking => {
            delta.push(AssistantSemanticEvent::ThinkingText { text, source })
        }
    }
}

pub(super) fn parse_transport_event(
    turn: &mut InFlightAssistantTurn,
    event: AssistantTransportEvent,
) -> AssistantTurnDelta {
    let mut delta = AssistantTurnDelta::default();

    if event.kind == AssistantSegmentKind::Thinking {
        push_text_event(
            &mut delta,
            AssistantSegmentKind::Thinking,
            event.text,
            ThinkingSource::Sidecar,
        );
        return delta;
    }

    delta.push(AssistantSemanticEvent::RawTransportText(event.text.clone()));

    if turn.pending_invocation.is_some() {
        return delta;
    }

    if turn.in_thinking_block {
        turn.pending_thinking_buffer.push_str(&event.text);
    } else {
        turn.pending_visible_buffer.push_str(&event.text);
    }

    loop {
        if turn.in_thinking_block {
            if let Some(end_offset) = turn.pending_thinking_buffer.find("</think>") {
                push_text_event(
                    &mut delta,
                    AssistantSegmentKind::Thinking,
                    turn.pending_thinking_buffer[..end_offset].to_string(),
                    ThinkingSource::Inline,
                );

                let remainder =
                    turn.pending_thinking_buffer[end_offset + "</think>".len()..].to_string();
                turn.pending_thinking_buffer.clear();
                turn.in_thinking_block = false;
                turn.pending_visible_buffer.push_str(&remainder);
                continue;
            }

            if let Some(partial_offset) =
                trailing_partial_marker_offset(&turn.pending_thinking_buffer, "</think>")
            {
                if partial_offset > 0 {
                    push_text_event(
                        &mut delta,
                        AssistantSegmentKind::Thinking,
                        turn.pending_thinking_buffer[..partial_offset].to_string(),
                        ThinkingSource::Inline,
                    );
                    turn.pending_thinking_buffer.drain(..partial_offset);
                }
            } else {
                let text = std::mem::take(&mut turn.pending_thinking_buffer);
                push_text_event(
                    &mut delta,
                    AssistantSegmentKind::Thinking,
                    text,
                    ThinkingSource::Inline,
                );
            }
            break;
        }

        let think_offset = find_next_think_marker(&turn.pending_visible_buffer);
        let invocation_search = crate::text_invocation::find_first_prefixed_json_invocation(
            &turn.pending_visible_buffer,
            &["ACTION:", "TOOL:"],
        );

        match invocation_search {
            crate::text_invocation::PrefixedInvocationSearch::Parsed(found)
                if think_offset.is_none_or(|offset| found.start_offset < offset) =>
            {
                if found.start_offset > 0 {
                    push_text_event(
                        &mut delta,
                        AssistantSegmentKind::Message,
                        turn.pending_visible_buffer[..found.start_offset].to_string(),
                        ThinkingSource::Inline,
                    );
                    turn.pending_visible_buffer.drain(..found.start_offset);
                }

                turn.pending_visible_buffer.clear();
                delta.push(AssistantSemanticEvent::InvocationDetected(
                    found.parsed.raw_invocation.clone(),
                ));
                break;
            }
            crate::text_invocation::PrefixedInvocationSearch::Incomplete { start_offset }
                if think_offset.is_none_or(|offset| start_offset < offset) =>
            {
                if start_offset > 0 {
                    push_text_event(
                        &mut delta,
                        AssistantSegmentKind::Message,
                        turn.pending_visible_buffer[..start_offset].to_string(),
                        ThinkingSource::Inline,
                    );
                    turn.pending_visible_buffer.drain(..start_offset);
                }
                break;
            }
            _ => {}
        }

        if let Some(offset) = think_offset {
            if offset > 0 {
                push_text_event(
                    &mut delta,
                    AssistantSegmentKind::Message,
                    turn.pending_visible_buffer[..offset].to_string(),
                    ThinkingSource::Inline,
                );
            }

            let remainder = turn.pending_visible_buffer[offset + "<think>".len()..].to_string();
            turn.pending_visible_buffer.clear();
            turn.in_thinking_block = true;
            turn.pending_thinking_buffer.push_str(&remainder);
            continue;
        }

        let mut retained_offset =
            trailing_partial_syscall_marker_offset(&turn.pending_visible_buffer);
        if let Some(think_partial_offset) =
            trailing_partial_marker_offset(&turn.pending_visible_buffer, "<think>")
        {
            retained_offset = Some(retained_offset.map_or(think_partial_offset, |current| {
                current.min(think_partial_offset)
            }));
        }

        if let Some(start_offset) = retained_offset {
            if start_offset > 0 {
                push_text_event(
                    &mut delta,
                    AssistantSegmentKind::Message,
                    turn.pending_visible_buffer[..start_offset].to_string(),
                    ThinkingSource::Inline,
                );
                turn.pending_visible_buffer.drain(..start_offset);
            }
        } else {
            let text = std::mem::take(&mut turn.pending_visible_buffer);
            push_text_event(
                &mut delta,
                AssistantSegmentKind::Message,
                text,
                ThinkingSource::Inline,
            );
        }
        break;
    }

    delta
}
