use agentic_control_models::AssistantSegmentKind;

use std::collections::HashMap;

use agentic_control_models::KernelEvent;
use mio::{Poll, Token};

use crate::diagnostics::audit::{self, AuditContext};
use crate::orchestrator::Orchestrator;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::transport::Client;

use super::assistant_output::emit_assistant_timeline_output;
use super::turn_assembly::TurnAssemblyStore;

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_stream_chunk(
    runtime_registry: &RuntimeRegistry,
    scheduler: &mut ProcessScheduler,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    orchestrator: &mut Orchestrator,
    pid: u64,
    text: &str,
    first_chunk: bool,
    session_registry: &SessionRegistry,
    storage: &mut StorageService,
    turn_assembly: &mut TurnAssemblyStore,
    pending_events: &mut Vec<KernelEvent>,
) {
    let Some(runtime_id) = runtime_registry
        .runtime_id_for_pid(pid)
        .map(ToString::to_string)
    else {
        tracing::warn!(
            pid,
            "RUNTIME: dropping worker stream chunk for unknown runtime pid"
        );
        return;
    };

    let owner_id = scheduler
        .checked_out_process(pid)
        .map(|metadata| metadata.owner_id)
        .or_else(|| {
            runtime_registry
                .engine(&runtime_id)
                .and_then(|engine| engine.process_owner_id(pid))
        })
        .unwrap_or(0);

    if first_chunk {
        audit::record(
            storage,
            audit::REMOTE_FIRST_CHUNK_RECEIVED,
            format!("backend={} pid={}", runtime_id, pid),
            AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(&runtime_id),
            ),
        );
    }

    if text.is_empty() {
        return;
    }

    let fragment = turn_assembly.consume_stream_fragment(pid, AssistantSegmentKind::Message, text);

    if let Some(command) = fragment.syscall_command.as_deref() {
        tracing::info!(
            pid,
            owner_id,
            command,
            "OS: SysCall buffered from streaming output"
        );
    }

    emit_assistant_timeline_output(
        pid,
        owner_id,
        &fragment.segments,
        clients,
        poll,
        orchestrator,
        pending_events,
        "model_output_chunk",
    );
}
