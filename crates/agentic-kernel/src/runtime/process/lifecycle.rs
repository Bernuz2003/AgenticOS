use crate::runtimes::RuntimeRegistry;

pub(super) fn termination_reason_for_pid(runtime_registry: &RuntimeRegistry, pid: u64) -> String {
    runtime_registry
        .runtime_id_for_pid(pid)
        .and_then(|runtime_id| runtime_registry.engine(runtime_id))
        .and_then(|engine| engine.processes.get(&pid))
        .and_then(|process| process.termination_reason.clone())
        .unwrap_or_else(|| "completed".to_string())
}
