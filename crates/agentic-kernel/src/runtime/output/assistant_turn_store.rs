use std::collections::HashMap;

use agentic_control_models::AssistantSegmentKind;

use super::assistant_output::{
    consume_assistant_output_fragment, AssistantOutputFragment, AssistantOutputSegment,
    InFlightAssistantTurn,
};

#[derive(Debug, Default, Clone)]
struct TurnAssemblyState {
    turn: InFlightAssistantTurn,
    pending_segments: Vec<AssistantOutputSegment>,
    output_stop_requested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssistantTurnRuntimeBoundary {
    AwaitingCanonicalCommit,
    CanonicalCommitApplied,
    RuntimeClosed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedInferencePrompt {
    pub full_prompt: String,
    pub resident_prompt_suffix: String,
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
    pub generated_token_count: usize,
    pub syscall_command: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct InFlightAssistantTurnStore {
    by_pid: HashMap<u64, TurnAssemblyState>,
}

impl InFlightAssistantTurnStore {
    pub(crate) fn clear_pid(&mut self, pid: u64) {
        self.apply_runtime_boundary(pid, AssistantTurnRuntimeBoundary::RuntimeClosed);
    }

    pub(crate) fn reset_pid_output_state(&mut self, pid: u64) {
        self.apply_runtime_boundary(pid, AssistantTurnRuntimeBoundary::CanonicalCommitApplied);
    }

    pub(crate) fn apply_runtime_boundary(
        &mut self,
        pid: u64,
        boundary: AssistantTurnRuntimeBoundary,
    ) {
        match boundary {
            AssistantTurnRuntimeBoundary::AwaitingCanonicalCommit => {
                if let Some(state) = self.by_pid.get_mut(&pid) {
                    state.turn.mark_boundary_commit_pending();
                }
            }
            AssistantTurnRuntimeBoundary::CanonicalCommitApplied => {
                if let Some(state) = self.by_pid.get_mut(&pid) {
                    state.turn.clear_runtime_state();
                }
            }
            AssistantTurnRuntimeBoundary::RuntimeClosed => {
                self.by_pid.remove(&pid);
            }
        }
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
        let fragment = consume_assistant_output_fragment(&mut state.turn, kind, text);
        queue_pending_segments(&mut state.pending_segments, &fragment.segments);
        assembly_fragment(fragment)
    }

    pub(crate) fn request_output_stop(&mut self, pid: u64) {
        self.by_pid.entry(pid).or_default().output_stop_requested = true;
    }

    pub(crate) fn take_output_stop_request(&mut self, pid: u64) -> bool {
        let Some(state) = self.by_pid.get_mut(&pid) else {
            return false;
        };
        let requested = state.output_stop_requested;
        state.output_stop_requested = false;
        requested
    }

    pub(crate) fn output_stop_requested(&self, pid: u64) -> bool {
        self.by_pid
            .get(&pid)
            .is_some_and(|state| state.output_stop_requested)
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
                &mut state.turn,
                AssistantSegmentKind::Thinking,
                reasoning_text,
            );
            queue_pending_segments(&mut state.pending_segments, &reasoning_fragment.segments);
            final_segments.extend(reasoning_fragment.segments);
        }

        let fragment =
            consume_assistant_output_fragment(&mut state.turn, AssistantSegmentKind::Message, text);
        queue_pending_segments(&mut state.pending_segments, &fragment.segments);
        final_segments.extend(fragment.segments.clone());

        let complete_assistant_text = state.turn.visible_projection().to_string();
        let continuation_text = state.turn.raw_transport_text().to_string();
        let generated_token_count = state.turn.generated_token_count();
        let syscall_command = state
            .turn
            .pending_invocation()
            .map(ToString::to_string)
            .or(fragment.syscall_command.clone());

        FinalizedTurnStep {
            segments: final_segments,
            complete_assistant_text,
            continuation_text,
            generated_token_count,
            syscall_command,
        }
    }

    pub(crate) fn render_inference_prompt(
        &self,
        pid: u64,
        canonical_prompt: &str,
        resident_prompt_checkpoint_bytes: usize,
    ) -> RenderedInferencePrompt {
        let continuation = self
            .by_pid
            .get(&pid)
            .map(|state| state.turn.raw_transport_text())
            .unwrap_or_default();
        let full_prompt = if continuation.is_empty() {
            canonical_prompt.to_string()
        } else {
            let mut prompt = String::with_capacity(canonical_prompt.len() + continuation.len());
            prompt.push_str(canonical_prompt);
            prompt.push_str(continuation);
            prompt
        };
        let checkpoint = resident_prompt_checkpoint_bytes.min(full_prompt.len());
        let resident_prompt_suffix = if checkpoint >= canonical_prompt.len() {
            continuation[checkpoint - canonical_prompt.len()..].to_string()
        } else {
            let mut suffix = canonical_prompt[checkpoint..].to_string();
            suffix.push_str(continuation);
            suffix
        };

        RenderedInferencePrompt {
            full_prompt,
            resident_prompt_suffix,
        }
    }

    pub(crate) fn accumulate_generated_tokens(&mut self, pid: u64, token_count: usize) {
        if token_count == 0 {
            return;
        }
        self.by_pid
            .entry(pid)
            .or_default()
            .turn
            .accumulate_generated_tokens(token_count);
    }

    pub(crate) fn pending_syscall(&self, pid: u64) -> Option<&str> {
        self.by_pid
            .get(&pid)
            .and_then(|state| state.turn.pending_invocation())
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
