use agentic_control_models::AssistantSegmentKind;

use crate::process::ProcessState;
use crate::runtime::syscalls::SyscallDispatchOutcome;
use crate::runtime::{should_emit_session_finished, TurnAssemblyStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamAssemblyObservation {
    pub segments: Vec<(AssistantSegmentKind, String)>,
    pub syscall_command: Option<String>,
    pub pending_syscall: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalAssemblyObservation {
    pub segments: Vec<(AssistantSegmentKind, String)>,
    pub syscall_command: Option<String>,
    pub complete_assistant_text: String,
    pub continuation_text: String,
    pub pending_segments: Vec<(AssistantSegmentKind, String)>,
}

#[derive(Debug, Default)]
pub struct TurnAssemblyHarness {
    store: TurnAssemblyStore,
    pid: u64,
}

impl TurnAssemblyHarness {
    pub fn new() -> Self {
        Self {
            store: TurnAssemblyStore::default(),
            pid: 1,
        }
    }

    pub fn push_stream(&mut self, text: &str) -> StreamAssemblyObservation {
        let fragment =
            self.store
                .consume_stream_fragment(self.pid, AssistantSegmentKind::Message, text);
        StreamAssemblyObservation {
            segments: fragment
                .segments
                .into_iter()
                .map(|segment| (segment.kind, segment.text))
                .collect(),
            syscall_command: fragment.syscall_command,
            pending_syscall: self
                .store
                .pending_syscall(self.pid)
                .map(ToString::to_string),
        }
    }

    pub fn finish_step(&mut self, text: &str) -> FinalAssemblyObservation {
        let finalized = self.store.consume_final_fragments(self.pid, text, "");
        FinalAssemblyObservation {
            segments: finalized
                .segments
                .into_iter()
                .map(|segment| (segment.kind, segment.text))
                .collect(),
            syscall_command: finalized.syscall_command,
            complete_assistant_text: finalized.complete_assistant_text,
            continuation_text: finalized.continuation_text,
            pending_segments: self
                .store
                .drain_pending_segments(self.pid)
                .unwrap_or_default()
                .into_iter()
                .map(|segment| (segment.kind, segment.text))
                .collect(),
        }
    }

    pub fn clear(&mut self) {
        self.store.clear_pid(self.pid);
    }

    pub fn reset_output_state(&mut self) {
        self.store.reset_pid_output_state(self.pid);
    }
}

pub fn should_emit_turn_completion(state: Option<&str>, syscall_queued: bool) -> bool {
    let turn_state = state.map(|value| match value {
        "WaitingForInput" => ProcessState::WaitingForInput,
        "WaitingForHumanInput" => ProcessState::WaitingForHumanInput,
        "AwaitingTurnDecision" => ProcessState::AwaitingTurnDecision,
        other => panic!("unsupported process state for test helper: {other}"),
    });
    let syscall_dispatch = if syscall_queued {
        SyscallDispatchOutcome::Queued
    } else {
        SyscallDispatchOutcome::None
    };
    should_emit_session_finished(turn_state.as_ref(), syscall_dispatch)
}
