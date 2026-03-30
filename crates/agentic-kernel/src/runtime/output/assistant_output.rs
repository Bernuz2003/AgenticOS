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

fn line_start_syscall_marker_offset(buffer: &str) -> Option<usize> {
    let mut absolute_offset = 0usize;
    for line in buffer.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let leading_ws = line.len() - trimmed.len();
        if trimmed.starts_with("ACTION:") || trimmed.starts_with("TOOL:") {
            return Some(absolute_offset + leading_ws);
        }
        absolute_offset += line.len();
    }

    let trimmed = buffer.trim_start();
    let leading_ws = buffer.len().saturating_sub(trimmed.len());
    if trimmed.starts_with("ACTION:") || trimmed.starts_with("TOOL:") {
        return Some(leading_ws);
    }

    None
}

fn malformed_line_start_syscall(buffer: &str) -> Option<String> {
    let marker_offset = line_start_syscall_marker_offset(buffer)?;
    let candidate = &buffer[marker_offset..];
    let prefix = if candidate.trim_start().starts_with("ACTION:") {
        "ACTION:"
    } else {
        "TOOL:"
    };

    match crate::text_invocation::extract_prefixed_json_invocation(candidate.trim_start(), prefix) {
        crate::text_invocation::PrefixedInvocationExtract::Invalid(_) => Some(
            candidate
                .lines()
                .next()
                .unwrap_or_default()
                .trim_end_matches('\r')
                .to_string(),
        ),
        crate::text_invocation::PrefixedInvocationExtract::Parsed(_)
        | crate::text_invocation::PrefixedInvocationExtract::Incomplete => None,
    }
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
                if let Some(command) =
                    malformed_line_start_syscall(&accumulator.pending_output_buffer)
                {
                    accumulator.pending_stream_syscall = Some(command);
                    accumulator.pending_output_buffer.clear();
                    break;
                }

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

pub(super) fn should_emit_session_finished(
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

#[cfg(test)]
mod tests {
    use super::{
        consume_assistant_output_fragment, should_emit_session_finished,
        AssistantOutputAccumulator, AssistantOutputFragment,
    };
    use crate::process::ProcessState;
    use crate::runtime::syscalls::SyscallDispatchOutcome;

    #[test]
    fn plain_stream_text_is_forwarded_and_captured() {
        let mut accumulator = AssistantOutputAccumulator::default();

        let fragment = consume_assistant_output_fragment(&mut accumulator, "ciao mondo");

        assert_eq!(
            fragment,
            AssistantOutputFragment {
                visible_text: "ciao mondo".to_string(),
                syscall_command: None,
            }
        );
        assert_eq!(accumulator.captured_assistant_text, "ciao mondo");
        assert!(accumulator.pending_output_buffer.is_empty());
        assert!(accumulator.pending_stream_syscall.is_none());
    }

    #[test]
    fn partial_tool_stream_is_withheld_until_complete() {
        let mut accumulator = AssistantOutputAccumulator::default();

        let first =
            consume_assistant_output_fragment(&mut accumulator, r#"TOOL:read_file {"path":"notes"#);
        assert_eq!(first.visible_text, "");
        assert!(first.syscall_command.is_none());
        assert_eq!(
            accumulator.pending_output_buffer,
            r#"TOOL:read_file {"path":"notes"#
        );

        let second = consume_assistant_output_fragment(&mut accumulator, r#"/todo.md"}"#);
        assert_eq!(second.visible_text, "");
        assert_eq!(
            second.syscall_command.as_deref(),
            Some(r#"TOOL:read_file {"path":"notes/todo.md"}"#)
        );
        assert!(accumulator.pending_output_buffer.is_empty());
    }

    #[test]
    fn text_before_tool_is_emitted_but_tool_itself_is_hidden() {
        let mut accumulator = AssistantOutputAccumulator::default();

        let fragment = consume_assistant_output_fragment(
            &mut accumulator,
            "Analizzo il file.\nTOOL:read_file {\"path\":\"doc.txt\"}",
        );

        assert_eq!(fragment.visible_text, "Analizzo il file.\n");
        assert_eq!(
            fragment.syscall_command.as_deref(),
            Some(r#"TOOL:read_file {"path":"doc.txt"}"#)
        );
        assert_eq!(accumulator.captured_assistant_text, "Analizzo il file.\n");
    }

    #[test]
    fn inline_tool_invocation_is_extracted_after_visible_preamble() {
        let mut accumulator = AssistantOutputAccumulator::default();

        let fragment = consume_assistant_output_fragment(
            &mut accumulator,
            r#"Creo la cartella richiesta: TOOL:mkdir {"path":"prova"}"#,
        );

        assert_eq!(fragment.visible_text, "Creo la cartella richiesta: ");
        assert_eq!(
            fragment.syscall_command.as_deref(),
            Some(r#"TOOL:mkdir {"path":"prova"}"#)
        );
        assert_eq!(
            accumulator.captured_assistant_text,
            "Creo la cartella richiesta: "
        );
    }

    #[test]
    fn invalid_inline_mention_does_not_block_later_valid_tool_invocation() {
        let mut accumulator = AssistantOutputAccumulator::default();

        let fragment = consume_assistant_output_fragment(
            &mut accumulator,
            "Uso la funzione TOOL:mkdir. Ecco la chiamata:\nTOOL:mkdir {\"path\":\"prova\"}",
        );

        assert_eq!(
            fragment.visible_text,
            "Uso la funzione TOOL:mkdir. Ecco la chiamata:\n"
        );
        assert_eq!(
            fragment.syscall_command.as_deref(),
            Some(r#"TOOL:mkdir {"path":"prova"}"#)
        );
    }

    #[test]
    fn incomplete_inline_tool_retains_only_pending_command_suffix() {
        let mut accumulator = AssistantOutputAccumulator::default();

        let fragment = consume_assistant_output_fragment(
            &mut accumulator,
            r#"Creo la cartella: TOOL:mkdir {"path":"pro"#,
        );

        assert_eq!(fragment.visible_text, "Creo la cartella: ");
        assert!(fragment.syscall_command.is_none());
        assert_eq!(
            accumulator.pending_output_buffer,
            r#"TOOL:mkdir {"path":"pro"#
        );
    }

    #[test]
    fn split_tool_marker_is_buffered_across_stream_chunks() {
        let mut accumulator = AssistantOutputAccumulator::default();

        let first = consume_assistant_output_fragment(&mut accumulator, "Richiesta:\n\nTO");
        assert_eq!(first.visible_text, "Richiesta:\n\n");
        assert!(first.syscall_command.is_none());
        assert_eq!(accumulator.pending_output_buffer, "TO");

        let second = consume_assistant_output_fragment(&mut accumulator, "OL");
        assert_eq!(second.visible_text, "");
        assert!(second.syscall_command.is_none());
        assert_eq!(accumulator.pending_output_buffer, "TOOL");

        let third =
            consume_assistant_output_fragment(&mut accumulator, r#":mkdir {"path":"prova"}"#);
        assert_eq!(third.visible_text, "");
        assert_eq!(
            third.syscall_command.as_deref(),
            Some(r#"TOOL:mkdir {"path":"prova"}"#)
        );
    }

    #[test]
    fn queued_tool_dispatch_suppresses_turn_completed_events() {
        assert!(!should_emit_session_finished(
            Some(&ProcessState::WaitingForInput),
            SyscallDispatchOutcome::Queued,
        ));
        assert!(should_emit_session_finished(
            Some(&ProcessState::WaitingForHumanInput),
            SyscallDispatchOutcome::None,
        ));
    }
}
