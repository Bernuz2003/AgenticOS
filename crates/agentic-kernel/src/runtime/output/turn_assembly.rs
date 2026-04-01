use std::collections::HashMap;

use agentic_control_models::AssistantSegmentKind;

use super::assistant_output::{
    consume_assistant_output_fragment, AssistantOutputAccumulator, AssistantOutputFragment,
    AssistantOutputSegment,
};

#[derive(Debug, Default, Clone)]
struct TurnAssemblyState {
    accumulator: AssistantOutputAccumulator,
    pending_segments: Vec<AssistantOutputSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnAssemblyFragment {
    pub segments: Vec<AssistantOutputSegment>,
    pub syscall_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FinalizedTurnStep {
    pub segments: Vec<AssistantOutputSegment>,
    pub complete_assistant_text: String,
    pub continuation_text: String,
    pub syscall_command: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct TurnAssemblyStore {
    by_pid: HashMap<u64, TurnAssemblyState>,
}

impl TurnAssemblyStore {
    pub(crate) fn clear_pid(&mut self, pid: u64) {
        self.by_pid.remove(&pid);
    }

    pub(crate) fn reset_pid_output_state(&mut self, pid: u64) {
        let Some(state) = self.by_pid.get_mut(&pid) else {
            return;
        };
        state.accumulator = AssistantOutputAccumulator::default();
    }

    pub(crate) fn consume_stream_fragment(
        &mut self,
        pid: u64,
        kind: AssistantSegmentKind,
        text: &str,
    ) -> TurnAssemblyFragment {
        if text.is_empty() {
            return TurnAssemblyFragment {
                segments: Vec::new(),
                syscall_command: None,
            };
        }

        let state = self.by_pid.entry(pid).or_default();
        let fragment = consume_assistant_output_fragment(&mut state.accumulator, kind, text);
        queue_pending_segments(&mut state.pending_segments, &fragment.segments);
        assembly_fragment(fragment)
    }

    pub(crate) fn consume_final_fragments(
        &mut self,
        pid: u64,
        text: &str,
        reasoning_text: &str,
    ) -> FinalizedTurnStep {
        let state = self.by_pid.entry(pid).or_default();
        let mut final_segments = Vec::new();
        if !reasoning_text.is_empty() {
            let reasoning_fragment = consume_assistant_output_fragment(
                &mut state.accumulator,
                AssistantSegmentKind::Thinking,
                reasoning_text,
            );
            queue_pending_segments(&mut state.pending_segments, &reasoning_fragment.segments);
            final_segments.extend(reasoning_fragment.segments);
        }

        let fragment = consume_assistant_output_fragment(
            &mut state.accumulator,
            AssistantSegmentKind::Message,
            text,
        );
        queue_pending_segments(&mut state.pending_segments, &fragment.segments);
        final_segments.extend(fragment.segments.clone());

        let complete_assistant_text = state.accumulator.captured_assistant_text.clone();
        let continuation_text = state.accumulator.captured_raw_assistant_output.clone();
        let syscall_command = state
            .accumulator
            .pending_stream_syscall
            .clone()
            .or(fragment.syscall_command.clone());

        FinalizedTurnStep {
            segments: final_segments,
            complete_assistant_text,
            continuation_text,
            syscall_command,
        }
    }

    pub(crate) fn pending_syscall(&self, pid: u64) -> Option<&str> {
        self.by_pid
            .get(&pid)
            .and_then(|state| state.accumulator.pending_stream_syscall.as_deref())
    }

    pub(crate) fn drain_pending_segments(
        &mut self,
        pid: u64,
    ) -> Option<Vec<AssistantOutputSegment>> {
        let state = self.by_pid.get_mut(&pid)?;
        if state.pending_segments.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut state.pending_segments))
    }
}

fn queue_pending_segments(
    pending_segments: &mut Vec<AssistantOutputSegment>,
    new_segments: &[AssistantOutputSegment],
) {
    for segment in new_segments {
        if segment.text.is_empty() {
            continue;
        }

        if let Some(existing) = pending_segments.last_mut() {
            if existing.kind == segment.kind {
                existing.text.push_str(&segment.text);
                continue;
            }
        }

        pending_segments.push(segment.clone());
    }
}

fn assembly_fragment(fragment: AssistantOutputFragment) -> TurnAssemblyFragment {
    TurnAssemblyFragment {
        segments: fragment.segments,
        syscall_command: fragment.syscall_command,
    }
}
