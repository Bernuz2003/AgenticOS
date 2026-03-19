use std::collections::HashSet;

use crate::memory::NeuralMemory;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::session::SessionRegistry;
use crate::storage::StorageService;

use super::process_runtime::{
    kill_managed_process_with_session, release_process_resources_with_session,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessSignalResult {
    Deferred,
    Applied,
    NotFound,
    NoModelLoaded,
}

#[allow(clippy::too_many_arguments)]
pub fn request_process_termination_with_session(
    runtime_registry: &mut RuntimeRegistry,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    in_flight: &HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pid: u64,
) -> ProcessSignalResult {
    if in_flight.contains(&pid) {
        pending_kills.push(pid);
        return ProcessSignalResult::Deferred;
    }

    let Some(runtime_id) = runtime_registry
        .runtime_id_for_pid(pid)
        .map(ToString::to_string)
    else {
        return ProcessSignalResult::NoModelLoaded;
    };
    let terminated = {
        let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
            return ProcessSignalResult::NoModelLoaded;
        };
        engine.terminate_process(pid)
    };

    if terminated {
        let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
            return ProcessSignalResult::NoModelLoaded;
        };
        release_process_resources_with_session(
            engine,
            memory,
            scheduler,
            session_registry,
            storage,
            pid,
            "terminated",
        );
        if let Err(err) = runtime_registry.release_pid(storage, pid) {
            tracing::warn!(pid, %err, "PROCESS_CONTROL: failed to release runtime binding");
        }
        ProcessSignalResult::Applied
    } else {
        ProcessSignalResult::NotFound
    }
}

#[allow(clippy::too_many_arguments)]
pub fn request_process_kill_with_session(
    runtime_registry: &mut RuntimeRegistry,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    in_flight: &HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pid: u64,
) -> ProcessSignalResult {
    if in_flight.contains(&pid) {
        pending_kills.push(pid);
        return ProcessSignalResult::Deferred;
    }

    let Some(runtime_id) = runtime_registry
        .runtime_id_for_pid(pid)
        .map(ToString::to_string)
    else {
        return ProcessSignalResult::NoModelLoaded;
    };
    {
        let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
            return ProcessSignalResult::NoModelLoaded;
        };
        kill_managed_process_with_session(
            engine,
            memory,
            scheduler,
            session_registry,
            storage,
            pid,
            "killed",
        );
    }
    if let Err(err) = runtime_registry.release_pid(storage, pid) {
        tracing::warn!(pid, %err, "PROCESS_CONTROL: failed to release runtime binding");
    }
    ProcessSignalResult::Applied
}
