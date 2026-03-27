use crate::process::ProcessState;

pub(super) fn is_checkout_eligible(state: &ProcessState) -> bool {
    matches!(state, ProcessState::Ready | ProcessState::Running)
}

pub(super) fn checked_out_state_label(backend_class: &str) -> String {
    if backend_class == "remote_stateless" {
        "AwaitingRemoteResponse".to_string()
    } else {
        "InFlight".to_string()
    }
}
