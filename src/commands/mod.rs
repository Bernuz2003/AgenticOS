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

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::prompting::PromptFamily;
use crate::protocol::OpCode;
use crate::scheduler::ProcessScheduler;
use crate::transport::Client;

use self::context::CommandContext;
use self::metrics::record_command;

// Re-export for auto-checkpoint in main.rs.
pub(crate) use self::metrics::snapshot_metrics as snapshot_metrics_fn;

#[allow(clippy::too_many_arguments)]
pub fn execute_command(
    client: &mut Client,
    header: crate::protocol::CommandHeader,
    payload: Vec<u8>,
    memory: &Rc<RefCell<NeuralMemory>>,
    engine_state: &Arc<Mutex<Option<LLMEngine>>>,
    model_catalog: &mut ModelCatalog,
    active_family: &mut PromptFamily,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    client_id: usize,
    shutdown_requested: &Arc<AtomicBool>,
) {
    let mut ctx = CommandContext {
        client,
        memory,
        engine_state,
        model_catalog,
        active_family,
        scheduler,
        orchestrator,
        client_id,
        shutdown_requested,
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
    };

    if response.starts_with(b"+OK") {
        record_command(true);
    } else {
        record_command(false);
    }

    ctx.client.output_buffer.extend(response);
}
