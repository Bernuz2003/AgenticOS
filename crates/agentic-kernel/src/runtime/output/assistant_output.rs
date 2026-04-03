mod reducer;
mod transport;
mod types;

use std::collections::HashMap;

use agentic_control_models::{AssistantSegmentKind, KernelEvent};
use mio::{Interest, Poll, Token};

use crate::orchestrator::Orchestrator;
use crate::process::ProcessState;
use crate::protocol;
use crate::runtime::syscalls::SyscallDispatchOutcome;
use crate::transport::Client;

use self::reducer::apply_turn_delta;
use self::transport::parse_transport_event;
use self::types::AssistantTransportEvent;
pub(super) use self::types::{
    AssistantOutputFragment, AssistantOutputSegment, InFlightAssistantTurn,
};

pub(super) fn consume_assistant_output_fragment(
    turn: &mut InFlightAssistantTurn,
    kind: AssistantSegmentKind,
    fragment: &str,
) -> AssistantOutputFragment {
    let Some(event) = AssistantTransportEvent::from_fragment(kind, fragment) else {
        return AssistantOutputFragment::default();
    };
    let delta = parse_transport_event(turn, event);
    apply_turn_delta(turn, delta)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_assistant_timeline_output(
    pid: u64,
    owner_id: usize,
    segments: &[AssistantOutputSegment],
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    orchestrator: &mut Orchestrator,
    pending_events: &mut Vec<KernelEvent>,
    reason: &str,
) {
    if segments.is_empty() {
        return;
    }

    let mut emitted_any = false;
    for segment in segments.iter().filter(|segment| !segment.text.is_empty()) {
        emitted_any = true;
        if segment.kind == AssistantSegmentKind::Message {
            if owner_id > 0 {
                let token = Token(owner_id);
                if let Some(client) = clients.get_mut(&token) {
                    client
                        .output_buffer
                        .extend(protocol::response_data(segment.text.as_bytes()));
                    let _ = poll.registry().reregister(
                        &mut client.stream,
                        token,
                        Interest::READABLE | Interest::WRITABLE,
                    );
                }
            }

            if orchestrator.is_orchestrated(pid) {
                orchestrator.append_output(pid, &segment.text);
            }
        }

        pending_events.push(KernelEvent::TimelineSegment {
            pid,
            segment_kind: segment.kind.clone(),
            text: segment.text.clone(),
        });
    }

    if emitted_any {
        pending_events.push(KernelEvent::WorkspaceChanged {
            pid,
            reason: reason.to_string(),
        });
    }
}

pub(crate) fn should_emit_session_finished(
    turn_state: Option<&ProcessState>,
    syscall_dispatch: SyscallDispatchOutcome,
) -> bool {
    if matches!(syscall_dispatch, SyscallDispatchOutcome::Queued) {
        return false;
    }

    matches!(
        turn_state,
        Some(
            ProcessState::WaitingForInput
                | ProcessState::WaitingForHumanInput
                | ProcessState::AwaitingTurnDecision
        )
    )
}
