use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::WorkloadClass;
use crate::scheduler::{ProcessPriority, ProcessScheduler};

pub struct ManagedProcessSpawn {
    pub pid: u64,
}

pub fn free_backend_slot_if_known(engine: &mut LLMEngine, memory: &NeuralMemory, pid: u64) {
    let Some(slot_id) = memory.slot_for_pid(pid) else {
        return;
    };

    if let Err(err) = engine.free_context_slot(slot_id) {
        tracing::debug!(pid, slot_id, %err, "MEMORY: backend slot free not available");
    }
}

pub fn release_process_resources(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    pid: u64,
) {
    free_backend_slot_if_known(engine, memory, pid);
    let _ = memory.release_process(pid);
    scheduler.unregister(pid);
}

pub fn kill_managed_process(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    pid: u64,
) {
    release_process_resources(engine, memory, scheduler, pid);
    engine.kill_process(pid);
}

pub fn spawn_managed_process(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    prompt: &str,
    owner_id: usize,
    workload: WorkloadClass,
    priority: ProcessPriority,
) -> Result<ManagedProcessSpawn, String> {
    let pid = engine
        .spawn_process(prompt, 0, owner_id)
        .map_err(|e| e.to_string())?;

    if let Some(token_slots) = engine.process_max_tokens(pid) {
        match memory.register_process(pid, token_slots) {
            Ok(slot_id) => {
                if let Err(err) = engine.set_process_context_slot(pid, slot_id) {
                    let _ = memory.release_process(pid);
                    engine.kill_process(pid);
                    return Err(err.to_string());
                }
            }
            Err(err) => {
                engine.kill_process(pid);
                return Err(err.to_string());
            }
        }
    }

    scheduler.register(pid, workload, priority);
    Ok(ManagedProcessSpawn { pid })
}
