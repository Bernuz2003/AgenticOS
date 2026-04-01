use std::collections::HashMap;

use agentic_control_models::{AssistantSegmentKind, KernelEvent};
use mio::{Interest, Poll, Token};

use crate::orchestrator::Orchestrator;
use crate::process::ProcessState;
use crate::protocol;
use crate::runtime::syscalls::SyscallDispatchOutcome;
use crate::transport::Client;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct AssistantOutputAccumulator {
    pub(super) pending_visible_buffer: String,
    pub(super) pending_thinking_buffer: String,
    pub(super) captured_assistant_text: String,
    pub(super) captured_raw_assistant_output: String,
    pub(super) pending_stream_syscall: Option<String>,
    pub(super) in_thinking_block: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AssistantOutputSegment {
    pub(crate) kind: AssistantSegmentKind,
    pub(crate) text: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct AssistantOutputFragment {
    pub(super) segments: Vec<AssistantOutputSegment>,
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

fn push_segment(
    accumulator: &mut AssistantOutputAccumulator,
    segments: &mut Vec<AssistantOutputSegment>,
    kind: AssistantSegmentKind,
    text: String,
) {
    if text.is_empty() {
        return;
    }

    if kind == AssistantSegmentKind::Message {
        accumulator.captured_assistant_text.push_str(&text);
    }

    segments.push(AssistantOutputSegment { kind, text });
}

#[allow(clippy::never_loop)]
pub(super) fn consume_assistant_output_fragment(
    accumulator: &mut AssistantOutputAccumulator,
    kind: AssistantSegmentKind,
    fragment: &str,
) -> AssistantOutputFragment {
    if fragment.is_empty() {
        return AssistantOutputFragment::default();
    }

    if kind == AssistantSegmentKind::Thinking {
        return AssistantOutputFragment {
            segments: vec![AssistantOutputSegment {
                kind,
                text: fragment.to_string(),
            }],
            syscall_command: accumulator.pending_stream_syscall.clone(),
        };
    }

    accumulator.captured_raw_assistant_output.push_str(fragment);

    if accumulator.pending_stream_syscall.is_some() {
        return AssistantOutputFragment::default();
    }

    if accumulator.in_thinking_block {
        accumulator.pending_thinking_buffer.push_str(fragment);
    } else {
        accumulator.pending_visible_buffer.push_str(fragment);
    }

    let mut segments = Vec::new();

    loop {
        if accumulator.in_thinking_block {
            if let Some(end_offset) = accumulator.pending_thinking_buffer.find("</think>") {
                push_segment(
                    accumulator,
                    &mut segments,
                    AssistantSegmentKind::Thinking,
                    accumulator.pending_thinking_buffer[..end_offset].to_string(),
                );

                let remainder = accumulator.pending_thinking_buffer
                    [end_offset + "</think>".len()..]
                    .to_string();
                accumulator.pending_thinking_buffer.clear();
                accumulator.in_thinking_block = false;
                accumulator.pending_visible_buffer.push_str(&remainder);
                continue;
            }

            if let Some(partial_offset) =
                trailing_partial_marker_offset(&accumulator.pending_thinking_buffer, "</think>")
            {
                if partial_offset > 0 {
                    push_segment(
                        accumulator,
                        &mut segments,
                        AssistantSegmentKind::Thinking,
                        accumulator.pending_thinking_buffer[..partial_offset].to_string(),
                    );
                    accumulator.pending_thinking_buffer.drain(..partial_offset);
                }
            } else {
                let text = std::mem::take(&mut accumulator.pending_thinking_buffer);
                push_segment(
                    accumulator,
                    &mut segments,
                    AssistantSegmentKind::Thinking,
                    text,
                );
            }
            break;
        }

        let think_offset = find_next_think_marker(&accumulator.pending_visible_buffer);
        let invocation_search = crate::text_invocation::find_first_prefixed_json_invocation(
            &accumulator.pending_visible_buffer,
            &["ACTION:", "TOOL:"],
        );

        match invocation_search {
            crate::text_invocation::PrefixedInvocationSearch::Parsed(found)
                if think_offset.is_none_or(|offset| found.start_offset < offset) =>
            {
                if found.start_offset > 0 {
                    push_segment(
                        accumulator,
                        &mut segments,
                        AssistantSegmentKind::Message,
                        accumulator.pending_visible_buffer[..found.start_offset].to_string(),
                    );
                    accumulator
                        .pending_visible_buffer
                        .drain(..found.start_offset);
                }

                accumulator.pending_stream_syscall = Some(found.parsed.raw_invocation.clone());
                accumulator.pending_visible_buffer.clear();
                break;
            }
            crate::text_invocation::PrefixedInvocationSearch::Incomplete { start_offset }
                if think_offset.is_none_or(|offset| start_offset < offset) =>
            {
                if start_offset > 0 {
                    push_segment(
                        accumulator,
                        &mut segments,
                        AssistantSegmentKind::Message,
                        accumulator.pending_visible_buffer[..start_offset].to_string(),
                    );
                    accumulator.pending_visible_buffer.drain(..start_offset);
                }
                break;
            }
            _ => {}
        }

        if let Some(offset) = think_offset {
            if offset > 0 {
                push_segment(
                    accumulator,
                    &mut segments,
                    AssistantSegmentKind::Message,
                    accumulator.pending_visible_buffer[..offset].to_string(),
                );
            }

            let remainder =
                accumulator.pending_visible_buffer[offset + "<think>".len()..].to_string();
            accumulator.pending_visible_buffer.clear();
            accumulator.in_thinking_block = true;
            accumulator.pending_thinking_buffer.push_str(&remainder);
            continue;
        }

        let mut retained_offset =
            trailing_partial_syscall_marker_offset(&accumulator.pending_visible_buffer);
        if let Some(think_partial_offset) =
            trailing_partial_marker_offset(&accumulator.pending_visible_buffer, "<think>")
        {
            retained_offset = Some(retained_offset.map_or(think_partial_offset, |current| {
                current.min(think_partial_offset)
            }));
        }

        if let Some(start_offset) = retained_offset {
            if start_offset > 0 {
                push_segment(
                    accumulator,
                    &mut segments,
                    AssistantSegmentKind::Message,
                    accumulator.pending_visible_buffer[..start_offset].to_string(),
                );
                accumulator.pending_visible_buffer.drain(..start_offset);
            }
        } else {
            let text = std::mem::take(&mut accumulator.pending_visible_buffer);
            push_segment(
                accumulator,
                &mut segments,
                AssistantSegmentKind::Message,
                text,
            );
        }
        break;
    }

    AssistantOutputFragment {
        segments,
        syscall_command: accumulator.pending_stream_syscall.clone(),
    }
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
