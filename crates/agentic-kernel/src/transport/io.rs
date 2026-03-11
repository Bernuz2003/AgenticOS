use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use agentic_control_models::KernelEvent;

use crate::commands::execute_command;
use crate::commands::MetricsState;
use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::protocol;
use crate::scheduler::ProcessScheduler;
use crate::tool_registry::ToolRegistry;

use super::{parse_available_commands, Client, ParsedCommand};

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn handle_read(
    client: &mut Client,
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
) -> bool {
    let mut tool_registry = ToolRegistry::with_builtins();
    let mut pending_events = Vec::new();
    handle_read_with_registry(
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
        &mut pending_events,
        metrics,
        &mut tool_registry,
        auth_token,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn handle_read_with_registry(
    client: &mut Client,
    memory: &mut NeuralMemory,
    engine_state: &mut Option<LLMEngine>,
    model_catalog: &mut ModelCatalog,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    client_id: usize,
    shutdown_requested: &Arc<AtomicBool>,
    in_flight: &HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pending_events: &mut Vec<KernelEvent>,
    metrics: &mut MetricsState,
    tool_registry: &mut ToolRegistry,
    auth_token: &str,
) -> bool {
    let mut chunk = [0; 4096];
    match client.stream.read(&mut chunk) {
        Ok(0) => return true,
        Ok(n) => {
            client.buffer.extend_from_slice(&chunk[..n]);
        }
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return false,
        Err(ref e)
            if e.kind() == io::ErrorKind::ConnectionReset
                || e.kind() == io::ErrorKind::BrokenPipe =>
        {
            return true;
        }
        Err(e) => {
            tracing::error!(%e, "Read error");
            return true;
        }
    }

    let parsed = parse_available_commands(&mut client.buffer, &mut client.state);
    for command in parsed {
        match command {
            ParsedCommand::Ok { header, payload } => execute_command(
                client,
                header,
                payload,
                memory,
                engine_state,
                model_catalog,
                scheduler,
                orchestrator,
                tool_registry,
                client_id,
                shutdown_requested,
                in_flight,
                pending_kills,
                pending_events,
                metrics,
                auth_token,
            ),
            ParsedCommand::Err(e) => {
                let request_id = client.allocate_request_id("transport");
                client.output_buffer.extend(protocol::response_protocol_err(
                    client,
                    &request_id,
                    "BAD_HEADER",
                    protocol::schema::ERROR,
                    &e,
                ));
            }
        }
    }
    false
}

pub fn handle_write(client: &mut Client) -> bool {
    while !client.output_buffer.is_empty() {
        let (head, _) = client.output_buffer.as_slices();
        match client.stream.write(head) {
            Ok(n) => {
                client.output_buffer.drain(..n);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return false,
            Err(_) => return true,
        }
    }
    false
}
