use agentic_control_models::{InvocationEvent, InvocationKind, InvocationStatus, KernelEvent};

pub(super) fn emit_invocation_updated(
    pending_events: &mut Vec<KernelEvent>,
    pid: u64,
    invocation_id: impl Into<String>,
    kind: InvocationKind,
    command: impl Into<String>,
    status: InvocationStatus,
) {
    pending_events.push(KernelEvent::InvocationUpdated {
        pid,
        invocation: InvocationEvent {
            invocation_id: invocation_id.into(),
            kind,
            command: command.into(),
            status,
        },
    });
}
