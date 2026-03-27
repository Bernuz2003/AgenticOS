use crate::engine::LLMEngine;
use crate::process::AgentProcess;
use crate::scheduler::CheckedOutProcessMetadata;

use super::assistant_output::{
    consume_assistant_output_fragment, AssistantOutputAccumulator, AssistantOutputFragment,
};

pub(super) fn consume_stream_output_fragment(
    checked_out: Option<&mut CheckedOutProcessMetadata>,
    text: &str,
) -> AssistantOutputFragment {
    if let Some(metadata) = checked_out {
        let mut accumulator = AssistantOutputAccumulator {
            pending_output_buffer: std::mem::take(&mut metadata.pending_output_buffer),
            captured_assistant_text: std::mem::take(&mut metadata.captured_assistant_text),
            pending_stream_syscall: metadata.pending_stream_syscall.take(),
        };
        let fragment = consume_assistant_output_fragment(&mut accumulator, text);
        metadata.pending_output_buffer = accumulator.pending_output_buffer;
        metadata.captured_assistant_text = accumulator.captured_assistant_text;
        metadata.pending_stream_syscall = accumulator.pending_stream_syscall;
        fragment
    } else {
        AssistantOutputFragment {
            visible_text: text.to_string(),
            syscall_command: None,
        }
    }
}

pub(super) fn output_accumulator_for_token(
    checked_out: Option<&CheckedOutProcessMetadata>,
    process: &AgentProcess,
) -> AssistantOutputAccumulator {
    AssistantOutputAccumulator {
        pending_output_buffer: checked_out
            .map(|metadata| metadata.pending_output_buffer.clone())
            .unwrap_or_else(|| process.syscall_buffer.clone()),
        captured_assistant_text: checked_out
            .map(|metadata| metadata.captured_assistant_text.clone())
            .unwrap_or_default(),
        pending_stream_syscall: checked_out.and_then(|metadata| metadata.pending_stream_syscall.clone()),
    }
}

pub(super) fn resolve_pending_syscall(
    engine: &mut LLMEngine,
    pid: u64,
    output_accumulator: &AssistantOutputAccumulator,
    final_fragment: &AssistantOutputFragment,
) -> Option<String> {
    output_accumulator
        .pending_stream_syscall
        .clone()
        .or(final_fragment.syscall_command.clone())
        .or_else(|| {
            if let Some(process) = engine.processes.get_mut(&pid) {
                crate::runtime::syscalls::scan_syscall_buffer(&mut process.syscall_buffer)
            } else {
                None
            }
        })
}
