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

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::protocol::OpCode;
use crate::scheduler::ProcessScheduler;
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
    engine_state: &mut Option<LLMEngine>,
    model_catalog: &mut ModelCatalog,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    client_id: usize,
    shutdown_requested: &Arc<AtomicBool>,
    in_flight: &HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    metrics: &mut MetricsState,
    auth_token: &str,
) {
    // ── C3: Auth gate — only AUTH and PING allowed before authentication ──
    if !client.authenticated && !matches!(header.opcode, OpCode::Auth | OpCode::Ping) {
        client
            .output_buffer
            .extend(crate::protocol::response_err_code("AUTH_REQUIRED", "Authenticate first with AUTH <token>"));
        return;
    }

    // Handle AUTH before creating CommandContext (avoids borrow conflict).
    if matches!(header.opcode, OpCode::Auth) {
        let token_attempt = String::from_utf8_lossy(&payload).trim().to_string();
        let response = if token_attempt == auth_token {
            client.authenticated = true;
            crate::protocol::response_ok_code("AUTH", "OK")
        } else {
            crate::protocol::response_err_code("AUTH_FAILED", "Invalid auth token")
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
        memory,
        engine_state,
        model_catalog,
        scheduler,
        orchestrator,
        client_id,
        shutdown_requested,
        in_flight,
        pending_kills,
        metrics,
    };

    // Handlers that may write directly to client.output_buffer and return None.
    let response = match header.opcode {
        OpCode::Ping => misc::handle_ping(),
        OpCode::Load => model::handle_load(&mut ctx, &payload),
        OpCode::ListModels => model::handle_list_models(&mut ctx),
        OpCode::SelectModel => model::handle_select_model(&mut ctx, &payload),
        OpCode::ModelInfo => model::handle_model_info(&mut ctx, &payload),
        OpCode::Exec => {
            if let Some(r) = exec::handle_exec(&mut ctx, &payload) { r } else { return; }
        }
        OpCode::Status => status::handle_status(&mut ctx, &payload),
        OpCode::Term => process_cmd::handle_term(&mut ctx, &payload),
        OpCode::Kill => process_cmd::handle_kill(&mut ctx, &payload),
        OpCode::Shutdown => misc::handle_shutdown(&mut ctx),
        OpCode::SetGen => misc::handle_set_gen(&mut ctx, &payload),
        OpCode::GetGen => misc::handle_get_gen(&mut ctx),
        OpCode::MemoryWrite => memory_cmd::handle_memory_write(&mut ctx, &payload),
        OpCode::SetPriority => scheduler_cmd::handle_set_priority(&mut ctx, &payload),
        OpCode::GetQuota => scheduler_cmd::handle_get_quota(&mut ctx, &payload),
        OpCode::SetQuota => scheduler_cmd::handle_set_quota(&mut ctx, &payload),
        OpCode::Checkpoint => checkpoint_cmd::handle_checkpoint(&mut ctx, &payload),
        OpCode::Restore => checkpoint_cmd::handle_restore(&mut ctx, &payload),
        OpCode::Orchestrate => {
            if let Some(r) = orchestration_cmd::handle_orchestrate(&mut ctx, &payload) { r } else { return; }
        }
        OpCode::Auth => unreachable!("AUTH handled above"),
    };

    if response.starts_with(b"+OK") {
        ctx.metrics.record_command(true);
    } else {
        ctx.metrics.record_command(false);
    }

    ctx.client.output_buffer.extend(response);
}
