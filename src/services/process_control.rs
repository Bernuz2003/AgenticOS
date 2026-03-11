use std::collections::HashSet;

use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::scheduler::ProcessScheduler;

use super::process_runtime::{kill_managed_process, release_process_resources};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessSignalResult {
    Deferred,
    Applied,
    NotFound,
    NoModelLoaded,
}

pub fn request_process_termination(
    engine_state: &mut Option<LLMEngine>,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    in_flight: &HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pid: u64,
) -> ProcessSignalResult {
    if in_flight.contains(&pid) {
        pending_kills.push(pid);
        return ProcessSignalResult::Deferred;
    }

    let Some(engine) = engine_state.as_mut() else {
        return ProcessSignalResult::NoModelLoaded;
    };

    if engine.terminate_process(pid) {
        release_process_resources(engine, memory, scheduler, pid);
        ProcessSignalResult::Applied
    } else {
        ProcessSignalResult::NotFound
    }
}

pub fn request_process_kill(
    engine_state: &mut Option<LLMEngine>,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    in_flight: &HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pid: u64,
) -> ProcessSignalResult {
    if in_flight.contains(&pid) {
        pending_kills.push(pid);
        return ProcessSignalResult::Deferred;
    }

    let Some(engine) = engine_state.as_mut() else {
        return ProcessSignalResult::NoModelLoaded;
    };

    kill_managed_process(engine, memory, scheduler, pid);
    ProcessSignalResult::Applied
}
