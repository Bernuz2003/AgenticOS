mod checkpoint_cmd;
mod context;
mod core_dump;
mod diagnostics;
mod exec;
mod memory_cmd;
mod misc;
mod models;
mod parsing;
#[path = "process/mod.rs"]
mod process_commands;
mod runtime;
mod tools_cmd;
#[path = "workflows/mod.rs"]
mod workflow_commands;

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use agentic_control_models::KernelEvent;
use agentic_protocol::ControlErrorCode;

use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::protocol::OpCode;
use crate::resource_governor::ResourceGovernor;
use crate::runtime::TurnAssemblyStore;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::services::job_scheduler::JobScheduler;
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::transport::Client;

use self::context::CommandContext;

// Re-export for other modules.
pub(crate) use self::context::{CoreDumpCommandContext, ProcessCommandContext};
pub(crate) use self::core_dump::{handle_core_dump, handle_core_dump_info, handle_list_core_dumps};
pub(crate) use self::diagnostics::MetricsState;
pub(crate) use self::process_commands::targeting::runtime_selector_for_session;
pub(crate) use self::process_commands::{handle_send_input, handle_stop_output};

#[allow(clippy::too_many_arguments)]
pub fn execute_command(
    client: &mut Client,
    header: crate::protocol::CommandHeader,
    payload: Vec<u8>,
    memory: &mut NeuralMemory,
    runtime_registry: &mut RuntimeRegistry,
    resource_governor: &mut ResourceGovernor,
    model_catalog: &mut ModelCatalog,
    scheduler: &mut ProcessScheduler,
    job_scheduler: &mut JobScheduler,
    orchestrator: &mut Orchestrator,
    tool_registry: &mut ToolRegistry,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    turn_assembly: &mut TurnAssemblyStore,
    client_id: usize,
    shutdown_requested: &Arc<AtomicBool>,
    in_flight: &HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pending_events: &mut Vec<KernelEvent>,
    metrics: &mut MetricsState,
    auth_token: &str,
    mcp_bridge: Option<&crate::mcp::bridge::McpBridgeRuntime>,
) {
    let request_id = client.allocate_request_id(&header.agent_id);

    // ── C3: Auth gate — only AUTH and PING allowed before authentication ──
    if !client.authenticated
        && !matches!(header.opcode, OpCode::Auth | OpCode::Ping | OpCode::Hello)
    {
        client
            .output_buffer
            .extend(crate::protocol::response_protocol_err_typed(
                client,
                &request_id,
                ControlErrorCode::AuthRequired,
                crate::protocol::schema::ERROR,
                "Authenticate first with AUTH <token>",
            ));
        return;
    }

    if matches!(header.opcode, OpCode::Hello) {
        let response = crate::protocol::handle_hello(client, &payload, &request_id);
        if response.starts_with(b"+OK") {
            metrics.record_command(true);
        } else {
            metrics.record_command(false);
        }
        client.output_buffer.extend(response);
        return;
    }

    // Handle AUTH before creating CommandContext (avoids borrow conflict).
    if matches!(header.opcode, OpCode::Auth) {
        let token_attempt = String::from_utf8_lossy(&payload).trim().to_string();
        let response = if token_attempt == auth_token {
            client.authenticated = true;
            crate::protocol::response_protocol_ok(
                client,
                &request_id,
                "AUTH",
                crate::protocol::schema::AUTH,
                &serde_json::json!({"status": "ok"}),
                Some("OK"),
            )
        } else {
            crate::protocol::response_protocol_err_typed(
                client,
                &request_id,
                ControlErrorCode::AuthFailed,
                crate::protocol::schema::ERROR,
                "Invalid auth token",
            )
        };
        if response.starts_with(b"+OK") {
            metrics.record_command(true);
        } else {
            metrics.record_command(false);
        }
        client.output_buffer.extend(response);
        return;
    }

    let mut ctx = CommandContext {
        client,
        request_id,
        memory,
        runtime_registry,
        resource_governor,
        model_catalog,
        scheduler,
        job_scheduler,
        orchestrator,
        tool_registry,
        session_registry,
        storage,
        turn_assembly,
        client_id,
        shutdown_requested,
        mcp_bridge,
        in_flight,
        pending_kills,
        pending_events,
        metrics,
    };

    // Handlers that may write directly to client.output_buffer and return None.
    let response = match header.opcode {
        OpCode::Ping => misc::handle_ping(ctx.misc_view()),
        OpCode::Subscribe => misc::handle_subscribe(ctx.misc_view()),
        OpCode::Load => models::handle_load(ctx.model_view(), &payload),
        OpCode::ListModels => models::handle_list_models(ctx.model_view()),
        OpCode::SelectModel => models::handle_select_model(ctx.model_view(), &payload),
        OpCode::ModelInfo => models::handle_model_info(ctx.model_view(), &payload),
        OpCode::BackendDiag => models::handle_backend_diag(ctx.model_view()),
        OpCode::Exec => {
            if let Some(r) = exec::handle_exec(ctx.exec_view(), &payload) {
                r
            } else {
                return;
            }
        }
        OpCode::ResumeSession => {
            self::process_commands::handle_resume_session(ctx.process_view(), &payload)
        }
        OpCode::ScheduleJob => {
            if let Some(r) =
                workflow_commands::jobs::handle_schedule_job(ctx.orchestration_view(), &payload)
            {
                r
            } else {
                return;
            }
        }
        OpCode::SetJobEnabled => {
            if let Some(r) =
                workflow_commands::jobs::handle_set_job_enabled(ctx.orchestration_view(), &payload)
            {
                r
            } else {
                return;
            }
        }
        OpCode::DeleteJob => {
            if let Some(r) =
                workflow_commands::jobs::handle_delete_job(ctx.orchestration_view(), &payload)
            {
                r
            } else {
                return;
            }
        }
        OpCode::SendInput => {
            self::process_commands::handle_send_input(ctx.process_view(), &payload)
        }
        OpCode::ContinueOutput => {
            self::process_commands::handle_continue_output(ctx.process_view(), &payload)
        }
        OpCode::StopOrchestration => {
            if let Some(r) = workflow_commands::orchestration::handle_stop_orchestration(
                ctx.orchestration_view(),
                &payload,
            ) {
                r
            } else {
                return;
            }
        }
        OpCode::DeleteOrchestration => {
            if let Some(r) = workflow_commands::orchestration::handle_delete_orchestration(
                ctx.orchestration_view(),
                &payload,
            ) {
                r
            } else {
                return;
            }
        }
        OpCode::StopOutput => {
            self::process_commands::handle_stop_output(ctx.process_view(), &payload)
        }
        OpCode::Status => runtime::handle_status(ctx.status_view(), &payload),
        OpCode::ListOrchestrations => {
            workflow_commands::control::handle_list_orchestrations(ctx.status_view(), &payload)
        }
        OpCode::OrchestrationStatus => {
            workflow_commands::control::handle_orchestration_status(ctx.status_view(), &payload)
        }
        OpCode::ListJobs => {
            workflow_commands::control::handle_list_jobs(ctx.status_view(), &payload)
        }
        OpCode::ListArtifacts => {
            workflow_commands::control::handle_list_artifacts(ctx.status_view(), &payload)
        }
        OpCode::ListCoreDumps => core_dump::handle_list_core_dumps(ctx.core_dump_view(), &payload),
        OpCode::ReplayCoreDump => core_dump::handle_replay_core_dump(ctx.process_view(), &payload),
        OpCode::Term => self::process_commands::handle_term(ctx.process_view(), &payload),
        OpCode::Kill => self::process_commands::handle_kill(ctx.process_view(), &payload),
        OpCode::Shutdown => misc::handle_shutdown(ctx.misc_view()),
        OpCode::SetGen => misc::handle_set_gen(ctx.misc_view(), &payload),
        OpCode::GetGen => misc::handle_get_gen(ctx.misc_view()),
        OpCode::MemoryWrite => memory_cmd::handle_memory_write(ctx.memory_view(), &payload),
        OpCode::SetPriority => {
            self::process_commands::lifecycle::handle_set_priority(ctx.scheduler_view(), &payload)
        }
        OpCode::GetQuota => {
            self::process_commands::lifecycle::handle_get_quota(ctx.scheduler_view(), &payload)
        }
        OpCode::SetQuota => {
            self::process_commands::lifecycle::handle_set_quota(ctx.scheduler_view(), &payload)
        }
        OpCode::Checkpoint => checkpoint_cmd::handle_checkpoint(ctx.checkpoint_view(), &payload),
        OpCode::CoreDump => core_dump::handle_core_dump(ctx.core_dump_view(), &payload),
        OpCode::CoreDumpInfo => core_dump::handle_core_dump_info(ctx.core_dump_view(), &payload),
        OpCode::Restore => checkpoint_cmd::handle_restore(ctx.checkpoint_view(), &payload),
        OpCode::Orchestrate => {
            if let Some(r) = workflow_commands::orchestration::handle_orchestrate(
                ctx.orchestration_view(),
                &payload,
            ) {
                r
            } else {
                return;
            }
        }
        OpCode::RetryTask => {
            if let Some(r) = workflow_commands::orchestration::handle_retry_task(
                ctx.orchestration_view(),
                &payload,
            ) {
                r
            } else {
                return;
            }
        }
        OpCode::ListTools => tools_cmd::handle_list_tools(ctx.tools_view()),
        OpCode::RegisterTool => tools_cmd::handle_register_tool(ctx.tools_view(), &payload),
        OpCode::ToolInfo => tools_cmd::handle_tool_info(ctx.tools_view(), &payload),
        OpCode::UnregisterTool => tools_cmd::handle_unregister_tool(ctx.tools_view(), &payload),
        OpCode::Hello => unreachable!("HELLO handled above"),
        OpCode::Auth => unreachable!("AUTH handled above"),
    };

    if response.starts_with(b"+OK") {
        ctx.metrics.record_command(true);
    } else {
        ctx.metrics.record_command(false);
    }

    ctx.client.output_buffer.extend(response);
}
