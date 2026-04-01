use std::collections::HashMap;

use agentic_control_models::KernelEvent;
use mio::{Interest, Poll, Token};

use crate::orchestrator::Orchestrator;
use crate::process::ProcessState;
use crate::protocol;
use crate::transport::Client;

use crate::runtime::syscalls::SyscallDispatchOutcome;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct AssistantOutputAccumulator {
    pub(super) pending_output_buffer: String,
    pub(super) captured_assistant_text: String,
    pub(super) pending_stream_syscall: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct AssistantOutputFragment {
    pub(super) visible_text: String,
    pub(super) syscall_command: Option<String>,
}

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

#[allow(clippy::never_loop)]
pub(super) fn consume_assistant_output_fragment(
    accumulator: &mut AssistantOutputAccumulator,
    fragment: &str,
) -> AssistantOutputFragment {
    if fragment.is_empty() {
        return AssistantOutputFragment::default();
    }

    if accumulator.pending_stream_syscall.is_some() {
        return AssistantOutputFragment::default();
    }

    accumulator.pending_output_buffer.push_str(fragment);
    let mut visible_text = String::new();

    loop {
        match crate::text_invocation::find_first_prefixed_json_invocation(
            &accumulator.pending_output_buffer,
            &["ACTION:", "TOOL:"],
        ) {
            crate::text_invocation::PrefixedInvocationSearch::Parsed(found) => {
                if found.start_offset > 0 {
                    visible_text.push_str(&accumulator.pending_output_buffer[..found.start_offset]);
                    accumulator
                        .pending_output_buffer
                        .drain(..found.start_offset);
                }

                accumulator.pending_stream_syscall = Some(found.parsed.raw_invocation.clone());
                accumulator.pending_output_buffer.clear();
                break;
            }
            crate::text_invocation::PrefixedInvocationSearch::Incomplete { start_offset } => {
                if start_offset > 0 {
                    visible_text.push_str(&accumulator.pending_output_buffer[..start_offset]);
                    accumulator.pending_output_buffer.drain(..start_offset);
                }
                break;
            }
            crate::text_invocation::PrefixedInvocationSearch::NotFound => {
                if let Some(start_offset) =
                    trailing_partial_syscall_marker_offset(&accumulator.pending_output_buffer)
                {
                    if start_offset > 0 {
                        visible_text.push_str(&accumulator.pending_output_buffer[..start_offset]);
                        accumulator.pending_output_buffer.drain(..start_offset);
                    }
                    break;
                }

                visible_text.push_str(&accumulator.pending_output_buffer);
                accumulator.pending_output_buffer.clear();
                break;
            }
        }
    }

    if !visible_text.is_empty() {
        accumulator.captured_assistant_text.push_str(&visible_text);
    }

    AssistantOutputFragment {
        visible_text,
        syscall_command: accumulator.pending_stream_syscall.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_visible_assistant_output(
    pid: u64,
    owner_id: usize,
    text: &str,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    orchestrator: &mut Orchestrator,
    pending_events: &mut Vec<KernelEvent>,
    reason: &str,
) {
    if text.is_empty() {
        return;
    }

    if owner_id > 0 {
        let token = Token(owner_id);
        if let Some(client) = clients.get_mut(&token) {
            client
                .output_buffer
                .extend(protocol::response_data(text.as_bytes()));
            let _ = poll.registry().reregister(
                &mut client.stream,
                token,
                Interest::READABLE | Interest::WRITABLE,
            );
        }
    }

    if orchestrator.is_orchestrated(pid) {
        orchestrator.append_output(pid, text);
    }
    pending_events.push(KernelEvent::TimelineChunk {
        pid,
        text: text.to_string(),
    });
    pending_events.push(KernelEvent::WorkspaceChanged {
        pid,
        reason: reason.to_string(),
    });
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
