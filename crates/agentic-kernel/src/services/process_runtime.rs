use crate::audit::{self, AuditContext};
use crate::backend::BackendClass;
use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::WorkloadClass;
use crate::process::{ContextPolicy, ProcessLifecyclePolicy};
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};

#[derive(Debug)]
pub struct ManagedProcessSpawn {
    pub session_id: String,
    pub runtime_id: String,
    pub pid: u64,
}

pub struct ManagedProcessRequest {
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub owner_id: usize,
    pub tool_caller: ToolCaller,
    pub permission_policy: Option<ProcessPermissionPolicy>,
    pub workload: WorkloadClass,
    pub required_backend_class: Option<BackendClass>,
    pub priority: ProcessPriority,
    pub lifecycle_policy: ProcessLifecyclePolicy,
    pub context_policy: Option<ContextPolicy>,
}

pub struct RestoredManagedProcessRequest {
    pub rendered_prompt: String,
    pub owner_id: usize,
    pub tool_caller: ToolCaller,
    pub permission_policy: Option<ProcessPermissionPolicy>,
    pub workload: WorkloadClass,
    pub required_backend_class: Option<BackendClass>,
    pub priority: ProcessPriority,
    pub lifecycle_policy: ProcessLifecyclePolicy,
    pub context_policy: Option<ContextPolicy>,
}

pub fn free_backend_slot_if_known(engine: &mut LLMEngine, memory: &NeuralMemory, pid: u64) {
    if let Err(err) = engine.free_process_context_slot(pid) {
        let Some(slot_id) = memory.slot_for_pid(pid) else {
            return;
        };

        if let Err(fallback_err) = engine.free_context_slot(slot_id) {
            tracing::debug!(
                pid,
                slot_id,
                primary_error = %err,
                fallback_error = %fallback_err,
                "MEMORY: backend slot free not available"
            );
        }
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

pub fn release_process_resources_with_session(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pid: u64,
    run_state: &str,
) {
    if let Err(err) = session_registry.release_pid(storage, pid, run_state) {
        tracing::warn!(pid, %err, "PROCESS_RUNTIME: failed to release session binding");
    }
    release_process_resources(engine, memory, scheduler, pid);
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

pub fn kill_managed_process_with_session(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    pid: u64,
    run_state: &str,
) {
    release_process_resources_with_session(
        engine,
        memory,
        scheduler,
        session_registry,
        storage,
        pid,
        run_state,
    );
    engine.kill_process(pid);
}

pub fn spawn_managed_process(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    request: ManagedProcessRequest,
) -> Result<ManagedProcessSpawn, String> {
    let ManagedProcessRequest {
        prompt,
        system_prompt,
        owner_id,
        tool_caller,
        permission_policy,
        workload,
        required_backend_class,
        priority,
        lifecycle_policy,
        context_policy,
    } = request;

    validate_backend_class_policy(engine.loaded_backend_class(), required_backend_class)?;

    let context_policy = context_policy.unwrap_or_else(ContextPolicy::from_kernel_defaults);
    let permission_policy = permission_policy
        .ok_or_else(|| "Missing process permission policy for spawn request.".to_string())?;
    let pid = engine
        .spawn_process(
            &prompt,
            system_prompt.as_deref(),
            0,
            owner_id,
            tool_caller,
            permission_policy,
            lifecycle_policy,
            context_policy,
        )
        .map_err(|e| e.to_string())?;

    attach_process_resources(engine, memory, scheduler, pid, workload, priority)?;
    Ok(ManagedProcessSpawn {
        session_id: format!("pid-{pid}"),
        runtime_id: String::new(),
        pid,
    })
}

pub fn spawn_restored_managed_process(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    request: RestoredManagedProcessRequest,
) -> Result<ManagedProcessSpawn, String> {
    let RestoredManagedProcessRequest {
        rendered_prompt,
        owner_id,
        tool_caller,
        permission_policy,
        workload,
        required_backend_class,
        priority,
        lifecycle_policy,
        context_policy,
    } = request;

    validate_backend_class_policy(engine.loaded_backend_class(), required_backend_class)?;

    let context_policy = context_policy.unwrap_or_else(ContextPolicy::from_kernel_defaults);
    let permission_policy = permission_policy
        .ok_or_else(|| "Missing process permission policy for restored request.".to_string())?;
    let pid = engine
        .restore_process_from_rendered_prompt(
            &rendered_prompt,
            owner_id,
            tool_caller,
            permission_policy,
            lifecycle_policy,
            context_policy,
        )
        .map_err(|err| err.to_string())?;

    attach_process_resources(engine, memory, scheduler, pid, workload, priority)?;
    Ok(ManagedProcessSpawn {
        session_id: format!("pid-{pid}"),
        runtime_id: String::new(),
        pid,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_managed_process_with_session(
    runtime_id: &str,
    pid_floor: u64,
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    request: ManagedProcessRequest,
) -> Result<ManagedProcessSpawn, String> {
    engine.ensure_next_pid_at_least(pid_floor);
    let session_id = session_registry
        .open_session(storage, &request.prompt, runtime_id)
        .map_err(|err| err.to_string())?;
    let request_workload = request.workload;
    let request_lifecycle = request.lifecycle_policy;

    match spawn_managed_process(engine, memory, scheduler, request) {
        Ok(mut spawned) => {
            if let Err(err) =
                session_registry.bind_pid(storage, &session_id, runtime_id, spawned.pid)
            {
                kill_managed_process(engine, memory, scheduler, spawned.pid);
                if let Err(cleanup_err) = session_registry.delete_session(storage, &session_id) {
                    tracing::warn!(
                        session_id,
                        pid = spawned.pid,
                        error = %cleanup_err,
                        "PROCESS_RUNTIME: failed to clean up session after bind failure"
                    );
                }
                return Err(err.to_string());
            }

            spawned.session_id = session_id;
            spawned.runtime_id = runtime_id.to_string();
            audit::record(
                storage,
                audit::PROCESS_SPAWNED,
                format!(
                    "pid={} runtime={} workload={:?} lifecycle={:?}",
                    spawned.pid, runtime_id, request_workload, request_lifecycle
                ),
                AuditContext::for_process(Some(&spawned.session_id), spawned.pid, Some(runtime_id)),
            );
            Ok(spawned)
        }
        Err(err) => {
            if let Err(cleanup_err) = session_registry.delete_session(storage, &session_id) {
                tracing::warn!(
                    session_id,
                    error = %cleanup_err,
                    "PROCESS_RUNTIME: failed to clean up session after spawn failure"
                );
            }
            Err(err)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_restored_managed_process_with_session(
    runtime_id: &str,
    session_id: &str,
    pid_floor: u64,
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    request: RestoredManagedProcessRequest,
) -> Result<ManagedProcessSpawn, String> {
    engine.ensure_next_pid_at_least(pid_floor);
    let request_workload = request.workload;
    let request_lifecycle = request.lifecycle_policy;

    match spawn_restored_managed_process(engine, memory, scheduler, request) {
        Ok(mut spawned) => {
            if let Err(err) =
                session_registry.bind_pid(storage, session_id, runtime_id, spawned.pid)
            {
                kill_managed_process(engine, memory, scheduler, spawned.pid);
                return Err(err.to_string());
            }

            spawned.session_id = session_id.to_string();
            spawned.runtime_id = runtime_id.to_string();
            audit::record(
                storage,
                audit::PROCESS_SPAWNED,
                format!(
                    "pid={} runtime={} workload={:?} lifecycle={:?} restored_from_history=true",
                    spawned.pid, runtime_id, request_workload, request_lifecycle
                ),
                AuditContext::for_process(Some(&spawned.session_id), spawned.pid, Some(runtime_id)),
            );
            Ok(spawned)
        }
        Err(err) => Err(err),
    }
}

fn validate_backend_class_policy(
    loaded_backend_class: BackendClass,
    required_backend_class: Option<BackendClass>,
) -> Result<(), String> {
    let Some(required_backend_class) = required_backend_class else {
        return Ok(());
    };

    if loaded_backend_class == required_backend_class {
        return Ok(());
    }

    Err(format!(
        "Process routing requires backend class '{}' but the loaded engine is '{}'.",
        required_backend_class.as_str(),
        loaded_backend_class.as_str()
    ))
}

fn attach_process_resources(
    engine: &mut LLMEngine,
    memory: &mut NeuralMemory,
    scheduler: &mut ProcessScheduler,
    pid: u64,
    workload: WorkloadClass,
    priority: ProcessPriority,
) -> Result<(), String> {
    let backend_capabilities = engine.loaded_backend_capabilities();
    let should_bind_resident_slot = backend_capabilities.resident_kv
        || backend_capabilities.persistent_slots
        || backend_capabilities.save_restore_slots;

    if should_bind_resident_slot {
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
    } else {
        tracing::info!(
            pid,
            backend_class = engine.loaded_backend_class().as_str(),
            "PROCESS_RUNTIME: skipping resident slot allocation for non-resident backend"
        );
    }

    scheduler.register(pid, workload, priority);
    Ok(())
}

#[cfg(test)]
#[path = "process_runtime_tests.rs"]
mod tests;
