mod checkpoint_cmd;
mod context;
mod exec;
mod memory_cmd;
mod metrics;
mod misc;
mod model;
mod orchestration_cmd;
mod parsing;
mod process_cmd;
mod scheduler_cmd;
mod status;
mod tools_cmd;
mod workflow_control;

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
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::services::job_scheduler::JobScheduler;
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::transport::Client;

use self::context::CommandContext;

// Re-export for other modules.
pub(crate) use self::metrics::MetricsState;

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
    client_id: usize,
    shutdown_requested: &Arc<AtomicBool>,
    in_flight: &HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pending_events: &mut Vec<KernelEvent>,
    metrics: &mut MetricsState,
    auth_token: &str,
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
        client_id,
        shutdown_requested,
        in_flight,
        pending_kills,
        pending_events,
        metrics,
    };

    // Handlers that may write directly to client.output_buffer and return None.
    let response = match header.opcode {
        OpCode::Ping => misc::handle_ping(ctx.misc_view()),
        OpCode::Subscribe => misc::handle_subscribe(ctx.misc_view()),
        OpCode::Load => model::handle_load(ctx.model_view(), &payload),
        OpCode::ListModels => model::handle_list_models(ctx.model_view()),
        OpCode::SelectModel => model::handle_select_model(ctx.model_view(), &payload),
        OpCode::ModelInfo => model::handle_model_info(ctx.model_view(), &payload),
        OpCode::BackendDiag => model::handle_backend_diag(ctx.model_view()),
        OpCode::Exec => {
            if let Some(r) = exec::handle_exec(ctx.exec_view(), &payload) {
                r
            } else {
                return;
            }
        }
        OpCode::ResumeSession => process_cmd::handle_resume_session(ctx.process_view(), &payload),
        OpCode::ScheduleJob => {
            if let Some(r) =
                orchestration_cmd::handle_schedule_job(ctx.orchestration_view(), &payload)
            {
                r
            } else {
                return;
            }
        }
        OpCode::SetJobEnabled => {
            if let Some(r) =
                orchestration_cmd::handle_set_job_enabled(ctx.orchestration_view(), &payload)
            {
                r
            } else {
                return;
            }
        }
        OpCode::DeleteJob => {
            if let Some(r) =
                orchestration_cmd::handle_delete_job(ctx.orchestration_view(), &payload)
            {
                r
            } else {
                return;
            }
        }
        OpCode::SendInput => process_cmd::handle_send_input(ctx.process_view(), &payload),
        OpCode::ContinueOutput => process_cmd::handle_continue_output(ctx.process_view(), &payload),
        OpCode::StopOrchestration => {
            if let Some(r) =
                orchestration_cmd::handle_stop_orchestration(ctx.orchestration_view(), &payload)
            {
                r
            } else {
                return;
            }
        }
        OpCode::DeleteOrchestration => {
            if let Some(r) =
                orchestration_cmd::handle_delete_orchestration(ctx.orchestration_view(), &payload)
            {
                r
            } else {
                return;
            }
        }
        OpCode::StopOutput => process_cmd::handle_stop_output(ctx.process_view(), &payload),
        OpCode::Status => status::handle_status(ctx.status_view(), &payload),
        OpCode::ListOrchestrations => {
            workflow_control::handle_list_orchestrations(ctx.status_view(), &payload)
        }
        OpCode::OrchestrationStatus => {
            workflow_control::handle_orchestration_status(ctx.status_view(), &payload)
        }
        OpCode::ListJobs => workflow_control::handle_list_jobs(ctx.status_view(), &payload),
        OpCode::ListArtifacts => {
            workflow_control::handle_list_artifacts(ctx.status_view(), &payload)
        }
        OpCode::Term => process_cmd::handle_term(ctx.process_view(), &payload),
        OpCode::Kill => process_cmd::handle_kill(ctx.process_view(), &payload),
        OpCode::Shutdown => misc::handle_shutdown(ctx.misc_view()),
        OpCode::SetGen => misc::handle_set_gen(ctx.misc_view(), &payload),
        OpCode::GetGen => misc::handle_get_gen(ctx.misc_view()),
        OpCode::MemoryWrite => memory_cmd::handle_memory_write(ctx.memory_view(), &payload),
        OpCode::SetPriority => scheduler_cmd::handle_set_priority(ctx.scheduler_view(), &payload),
        OpCode::GetQuota => scheduler_cmd::handle_get_quota(ctx.scheduler_view(), &payload),
        OpCode::SetQuota => scheduler_cmd::handle_set_quota(ctx.scheduler_view(), &payload),
        OpCode::Checkpoint => checkpoint_cmd::handle_checkpoint(ctx.checkpoint_view(), &payload),
        OpCode::Restore => checkpoint_cmd::handle_restore(ctx.checkpoint_view(), &payload),
        OpCode::Orchestrate => {
            if let Some(r) =
                orchestration_cmd::handle_orchestrate(ctx.orchestration_view(), &payload)
            {
                r
            } else {
                return;
            }
        }
        OpCode::RetryTask => {
            if let Some(r) =
                orchestration_cmd::handle_retry_task(ctx.orchestration_view(), &payload)
            {
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
