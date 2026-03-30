use std::collections::{HashMap, HashSet};

use agentic_control_models::KernelEvent;
use mio::{Poll, Token};

use crate::inference_worker::InferenceCmd;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::{Orchestrator, SpawnRequest};
use crate::process::ProcessLifecyclePolicy;
use crate::resource_governor::ResourceGovernor;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::{ProcessPriority, ProcessScheduler};
use crate::services::orchestration_runtime::resolve_runtime_for_spawn_request;
use crate::services::process_runtime::{spawn_managed_process_with_session, ManagedProcessRequest};
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};
use crate::transport::Client;

use super::task_runtime::{record_spawn_failure, record_spawn_start};

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_orchestrator_request(
    runtime_registry: &mut RuntimeRegistry,
    resource_governor: &mut ResourceGovernor,
    memory: &mut NeuralMemory,
    model_catalog: &mut ModelCatalog,
    _clients: &mut HashMap<Token, Client>,
    _poll: &Poll,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    _in_flight: &mut HashSet<u64>,
    _pending_kills: &mut [u64],
    pending_events: &mut Vec<KernelEvent>,
    _cmd_tx: &std::sync::mpsc::Sender<InferenceCmd>,
    tool_registry: &ToolRegistry,
    system_prompt: &str,
    req: SpawnRequest,
) {
    let permission_policy = match ProcessPermissionPolicy::workflow_supervisor(
        tool_registry,
        Some(&req.permission_overrides),
    ) {
        Ok(policy) => policy,
        Err(err) => {
            record_spawn_failure(
                storage,
                orchestrator,
                pending_events,
                req.orch_id,
                &req.task_id,
                req.attempt,
                &err,
            );
            tracing::error!(task_id = %req.task_id, %err, "ORCHESTRATOR: invalid task permissions");
            return;
        }
    };
    let runtime_id = match resolve_runtime_for_spawn_request(
        runtime_registry,
        resource_governor,
        storage,
        model_catalog,
        session_registry,
        &req,
    ) {
        Ok(runtime_id) => runtime_id,
        Err(err) => {
            let error = err.to_string();
            record_spawn_failure(
                storage,
                orchestrator,
                pending_events,
                req.orch_id,
                &req.task_id,
                req.attempt,
                &error,
            );
            tracing::error!(task_id = %req.task_id, %err, "ORCHESTRATOR: routing failed");
            return;
        }
    };

    let pid_floor = runtime_registry.next_pid_floor();
    let spawn_result = {
        let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
            let error = "resolved runtime has no loaded engine";
            record_spawn_failure(
                storage,
                orchestrator,
                pending_events,
                req.orch_id,
                &req.task_id,
                req.attempt,
                error,
            );
            return;
        };
        let effective_context_policy = req
            .context_policy
            .align_to_runtime_window_if_default(engine.effective_context_window_tokens());
        spawn_managed_process_with_session(
            &runtime_id,
            pid_floor,
            engine,
            memory,
            scheduler,
            session_registry,
            storage,
            ManagedProcessRequest {
                prompt: req.prompt.clone(),
                system_prompt: Some(system_prompt.to_string()),
                owner_id: req.owner_id,
                tool_caller: ToolCaller::AgentSupervisor,
                permission_policy: Some(permission_policy),
                workload: req.workload,
                required_backend_class: req.required_backend_class,
                priority: ProcessPriority::Normal,
                lifecycle_policy: ProcessLifecyclePolicy::Ephemeral,
                context_policy: Some(effective_context_policy),
            },
        )
    };

    match spawn_result {
        Ok(spawned_process) => {
            let pid = spawned_process.pid;
            if let Err(err) = runtime_registry.register_pid(storage, &runtime_id, pid) {
                tracing::warn!(
                    pid,
                    runtime_id,
                    %err,
                    "ORCHESTRATOR: failed to register spawned pid"
                );
            }
            record_spawn_start(
                storage,
                orchestrator,
                pending_events,
                &req,
                &spawned_process,
            );
            tracing::info!(
                pid,
                orch_id = req.orch_id,
                task_id = %req.task_id,
                "ORCHESTRATOR: spawned dependent task"
            );
        }
        Err(err) => {
            let error = err.to_string();
            record_spawn_failure(
                storage,
                orchestrator,
                pending_events,
                req.orch_id,
                &req.task_id,
                req.attempt,
                &error,
            );
            tracing::error!(task_id = %req.task_id, %err, "ORCHESTRATOR: spawn failed");
        }
    }
}
