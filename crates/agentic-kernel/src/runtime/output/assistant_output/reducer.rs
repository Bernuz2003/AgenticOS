use agentic_control_models::AssistantSegmentKind;

use super::types::{
    AssistantOutputFragment, AssistantOutputSegment, AssistantSemanticEvent, AssistantTurnDelta,
    InFlightAssistantTurn, ThinkingSource,
};

fn push_segment(
    turn: &mut InFlightAssistantTurn,
    segments: &mut Vec<AssistantOutputSegment>,
    kind: AssistantSegmentKind,
    text: String,
) {
    if text.is_empty() {
        return;
    }

    match kind {
        AssistantSegmentKind::Message => turn.visible_projection.push_str(&text),
        AssistantSegmentKind::Thinking => turn.thinking_projection.push_str(&text),
    }

    segments.push(AssistantOutputSegment { kind, text });
}

pub(super) fn apply_turn_delta(
    turn: &mut InFlightAssistantTurn,
    delta: AssistantTurnDelta,
) -> AssistantOutputFragment {
    let mut fragment = AssistantOutputFragment::default();

    for event in delta.events {
        match event {
            AssistantSemanticEvent::RawTransportText(text) => {
                turn.raw_transport_text.push_str(&text);
            }
            AssistantSemanticEvent::VisibleText(text) => {
                push_segment(
                    turn,
                    &mut fragment.segments,
                    AssistantSegmentKind::Message,
                    text,
                );
            }
            AssistantSemanticEvent::ThinkingText { text, source } => {
                if matches!(source, ThinkingSource::Sidecar) {
                    turn.reasoning_sidecar_projection.push_str(&text);
                }
                push_segment(
                    turn,
                    &mut fragment.segments,
                    AssistantSegmentKind::Thinking,
                    text,
                );
            }
            AssistantSemanticEvent::InvocationDetected(command) => {
                turn.pending_invocation = Some(command);
            }
        }
    }

    turn.recompute_phase();
    fragment.syscall_command = turn.pending_invocation.clone();
    fragment
}
