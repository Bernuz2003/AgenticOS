use std::collections::HashMap;

use super::assistant_output::{
    consume_assistant_output_fragment, AssistantOutputAccumulator, AssistantOutputFragment,
};

#[derive(Debug, Default, Clone)]
struct TurnAssemblyState {
    accumulator: AssistantOutputAccumulator,
    pending_assistant_segment: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnAssemblyFragment {
    pub visible_text: String,
    pub syscall_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FinalizedTurnStep {
    pub visible_text: String,
    pub complete_assistant_text: String,
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

    pub(crate) fn consume_stream_fragment(&mut self, pid: u64, text: &str) -> TurnAssemblyFragment {
        if text.is_empty() {
            return TurnAssemblyFragment {
                visible_text: String::new(),
                syscall_command: None,
            };
        }

        let state = self.by_pid.entry(pid).or_default();
        let fragment = consume_assistant_output_fragment(&mut state.accumulator, text);
        if !fragment.visible_text.is_empty() {
            state
                .pending_assistant_segment
                .push_str(&fragment.visible_text);
        }
        assembly_fragment(fragment)
    }

    pub(crate) fn consume_final_fragment(&mut self, pid: u64, text: &str) -> FinalizedTurnStep {
        let state = self.by_pid.entry(pid).or_default();
        let fragment = consume_assistant_output_fragment(&mut state.accumulator, text);
        if !fragment.visible_text.is_empty() {
            state
                .pending_assistant_segment
                .push_str(&fragment.visible_text);
        }

        let complete_assistant_text =
            std::mem::take(&mut state.accumulator.captured_assistant_text);
        let syscall_command = state
            .accumulator
            .pending_stream_syscall
            .take()
            .or(fragment.syscall_command.clone());

        FinalizedTurnStep {
            visible_text: fragment.visible_text,
            complete_assistant_text,
            syscall_command,
        }
    }

    pub(crate) fn pending_syscall(&self, pid: u64) -> Option<&str> {
        self.by_pid
            .get(&pid)
            .and_then(|state| state.accumulator.pending_stream_syscall.as_deref())
    }

    pub(crate) fn drain_pending_assistant_segment(&mut self, pid: u64) -> Option<String> {
        let state = self.by_pid.get_mut(&pid)?;
        if state.pending_assistant_segment.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut state.pending_assistant_segment))
    }
}

fn assembly_fragment(fragment: AssistantOutputFragment) -> TurnAssemblyFragment {
    TurnAssemblyFragment {
        visible_text: fragment.visible_text,
        syscall_command: fragment.syscall_command,
    }
}
