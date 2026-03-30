use agentic_control_models::KernelEvent;

use crate::diagnostics::audit::{self, AuditContext};
use crate::process::ProcessState;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::storage::StorageService;

use super::assistant_output::should_emit_session_finished;

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_turn_completion_events(
    runtime_registry: &RuntimeRegistry,
    scheduler: &ProcessScheduler,
    pid: u64,
    runtime_id: &str,
    syscall_dispatch: crate::runtime::syscalls::SyscallDispatchOutcome,
    pending_events: &mut Vec<KernelEvent>,
    storage: &mut StorageService,
    audit_context: AuditContext,
) {
    let turn_state = runtime_registry
        .engine(runtime_id)
        .and_then(|engine| engine.processes.get(&pid))
        .map(|process| process.state.clone());
    if !should_emit_session_finished(turn_state.as_ref(), syscall_dispatch) {
        return;
    }

    let sched = scheduler.snapshot(pid);
    let reason = if matches!(turn_state, Some(ProcessState::AwaitingTurnDecision)) {
        "awaiting_turn_decision"
    } else if matches!(turn_state, Some(ProcessState::WaitingForHumanInput)) {
        "human_input_requested"
    } else {
        "turn_completed"
    };
    pending_events.push(KernelEvent::SessionFinished {
        pid,
        tokens_generated: sched
            .as_ref()
            .map(|snapshot| snapshot.tokens_generated as u64),
        elapsed_secs: sched.as_ref().map(|snapshot| snapshot.elapsed_secs),
        reason: reason.to_string(),
    });
    pending_events.push(KernelEvent::WorkspaceChanged {
        pid,
        reason: reason.to_string(),
    });
    pending_events.push(KernelEvent::LobbyChanged {
        reason: reason.to_string(),
    });
    audit::record(
        storage,
        audit::PROCESS_TURN_COMPLETED,
        format!(
            "state={:?} tokens={} elapsed={:.3}s reason={}",
            turn_state,
            sched
                .as_ref()
                .map(|snapshot| snapshot.tokens_generated)
                .unwrap_or(0),
            sched
                .as_ref()
                .map(|snapshot| snapshot.elapsed_secs)
                .unwrap_or(0.0),
            reason
        ),
        audit_context,
    );
}
