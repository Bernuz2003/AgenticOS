use mio::{Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use agentic_control_models::KernelEvent;

use crate::core_dump::{
    core_dump_created_event, maybe_capture_automatic_core_dump, AutomaticCaptureKind,
    CaptureCoreDumpArgs,
};
use crate::inference_worker::InferenceCmd;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::resource_governor::ResourceGovernor;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::services::process_runtime::kill_managed_process_with_session;
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::transport::Client;
use crate::{diagnostics::audit, protocol};

use super::advance::collect_orchestrator_actions;
use super::spawn::spawn_orchestrator_request;

#[allow(clippy::too_many_arguments)]
pub(crate) fn advance_orchestrator(
    runtime_registry: &mut RuntimeRegistry,
    resource_governor: &mut ResourceGovernor,
    memory: &mut NeuralMemory,
    model_catalog: &mut ModelCatalog,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    turn_assembly: &crate::runtime::TurnAssemblyStore,
    in_flight: &mut HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pending_events: &mut Vec<KernelEvent>,
    cmd_tx: &mpsc::Sender<InferenceCmd>,
    tool_registry: &ToolRegistry,
) {
    let (spawn_requests, kill_pids) = collect_orchestrator_actions(orchestrator);
    let system_prompt = crate::agent_prompt::build_agent_system_prompt(
        tool_registry,
        crate::tools::invocation::ToolCaller::AgentSupervisor,
    );

    for pid in kill_pids {
        tracing::warn!(pid, "ORCHESTRATOR: killing task (fail_fast policy)");
        if in_flight.contains(&pid) {
            pending_kills.push(pid);
            continue;
        }
        let Some(runtime_id) = runtime_registry
            .runtime_id_for_pid(pid)
            .map(ToString::to_string)
        else {
            continue;
        };
        let owner_id = runtime_registry
            .engine(&runtime_id)
            .and_then(|engine| engine.process_owner_id(pid));
        if let Some(owner_id) = owner_id {
            if owner_id > 0 {
                let token = Token(owner_id);
                if let Some(client) = clients.get_mut(&token) {
                    let msg = format!("\n[ORCHESTRATOR_TASK_KILLED pid={}]\n", pid);
                    client
                        .output_buffer
                        .extend(protocol::response_data(msg.as_bytes()));
                    let _ = poll.registry().reregister(
                        &mut client.stream,
                        token,
                        Interest::READABLE | Interest::WRITABLE,
                    );
                }
            }
        }
        pending_events.push(KernelEvent::SessionFinished {
            pid,
            tokens_generated: None,
            elapsed_secs: None,
            reason: "orchestrator_killed".to_string(),
        });
        pending_events.push(KernelEvent::WorkspaceChanged {
            pid,
            reason: "orchestrator_killed".to_string(),
        });
        pending_events.push(KernelEvent::LobbyChanged {
            reason: "orchestrator_killed".to_string(),
        });
        let audit_context = audit::context_for_pid(session_registry, runtime_registry, pid);
        audit::record(
            storage,
            audit::PROCESS_KILLED,
            "reason=orchestrator_killed",
            audit_context,
        );
        match maybe_capture_automatic_core_dump(
            CaptureCoreDumpArgs {
                runtime_registry: &*runtime_registry,
                scheduler: &*scheduler,
                session_registry: &*session_registry,
                storage,
                turn_assembly,
                memory: &*memory,
                in_flight: &*in_flight,
            },
            pid,
            "orchestrator_fail_fast",
            Some("fail_fast orchestration kill".to_string()),
            AutomaticCaptureKind::Kill,
        ) {
            Ok(Some(summary)) => {
                if let Some(event) = core_dump_created_event(&summary) {
                    pending_events.push(event);
                }
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    pid,
                    %err,
                    "COREDUMP: automatic capture failed after orchestrator kill"
                );
            }
        }
        {
            let Some(engine) = runtime_registry.engine_mut(&runtime_id) else {
                continue;
            };
            kill_managed_process_with_session(
                engine,
                memory,
                scheduler,
                session_registry,
                storage,
                pid,
                "orchestrator_killed",
            );
        }
        if let Err(err) = runtime_registry.release_pid(storage, pid) {
            tracing::warn!(pid, %err, "ORCHESTRATOR: failed to release runtime binding on kill");
        }
    }

    for req in spawn_requests {
        spawn_orchestrator_request(
            runtime_registry,
            resource_governor,
            memory,
            model_catalog,
            clients,
            poll,
            scheduler,
            orchestrator,
            session_registry,
            storage,
            in_flight,
            pending_kills.as_mut_slice(),
            pending_events,
            cmd_tx,
            tool_registry,
            &system_prompt,
            req,
        );
    }
}
