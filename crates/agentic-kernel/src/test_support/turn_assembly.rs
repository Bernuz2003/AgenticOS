use crate::process::ProcessState;
use crate::runtime::syscalls::SyscallDispatchOutcome;
use crate::runtime::{should_emit_session_finished, TurnAssemblyStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamAssemblyObservation {
    pub visible_text: String,
    pub syscall_command: Option<String>,
    pub pending_syscall: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalAssemblyObservation {
    pub visible_text: String,
    pub syscall_command: Option<String>,
    pub complete_assistant_text: String,
    pub pending_assistant_segment: Option<String>,
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
        let fragment = self.store.consume_stream_fragment(self.pid, text);
        StreamAssemblyObservation {
            visible_text: fragment.visible_text,
            syscall_command: fragment.syscall_command,
            pending_syscall: self
                .store
                .pending_syscall(self.pid)
                .map(ToString::to_string),
        }
    }

    pub fn finish_step(&mut self, text: &str) -> FinalAssemblyObservation {
        let finalized = self.store.consume_final_fragment(self.pid, text);
        FinalAssemblyObservation {
            visible_text: finalized.visible_text,
            syscall_command: finalized.syscall_command,
            complete_assistant_text: finalized.complete_assistant_text,
            pending_assistant_segment: self.store.drain_pending_assistant_segment(self.pid),
        }
    }

    pub fn clear(&mut self) {
        self.store.clear_pid(self.pid);
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
