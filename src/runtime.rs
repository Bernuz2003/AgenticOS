mod inference_results;
mod orchestration;
mod syscalls;

use mio::{Poll, Token};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use crate::engine::LLMEngine;
use crate::inference_worker::{InferenceCmd, InferenceResult};
use crate::memory::NeuralMemory;
use crate::orchestrator::Orchestrator;
use crate::scheduler::ProcessScheduler;
use crate::tool_registry::ToolRegistry;
use crate::tools::SyscallRateMap;
use crate::transport::Client;

use inference_results::drain_worker_results;
use orchestration::{advance_orchestrator, checkout_active_processes, handle_finished_processes};

#[allow(clippy::too_many_arguments)]
pub fn run_engine_tick(
    engine_state: &mut Option<LLMEngine>,
    memory: &mut NeuralMemory,
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    cmd_tx: &mpsc::Sender<InferenceCmd>,
    result_rx: &mpsc::Receiver<InferenceResult>,
    in_flight: &mut HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    rate_map: &mut SyscallRateMap,
    tool_registry: &ToolRegistry,
) {
    if let Some(engine) = engine_state.as_mut() {
        let swap_events = memory.poll_swap_events();
        for event in swap_events {
            if event.success {
                let resumed = engine.set_process_ready_if_waiting(event.pid);
                tracing::info!(
                    pid = event.pid,
                    slot_id = event.slot_id,
                    resumed,
                    detail = %event.detail,
                    "MEMORY: swap complete"
                );
            } else {
                let resumed = engine.set_process_ready_if_waiting(event.pid);
                tracing::error!(
                    pid = event.pid,
                    slot_id = event.slot_id,
                    resumed,
                    detail = %event.detail,
                    "MEMORY: swap failed"
                );
            }
        }

        drain_worker_results(
            engine,
            memory,
            clients,
            poll,
            scheduler,
            orchestrator,
            result_rx,
            in_flight,
            pending_kills,
            rate_map,
            tool_registry,
        );

        handle_finished_processes(engine, memory, clients, poll, scheduler, orchestrator);
        checkout_active_processes(engine, scheduler, cmd_tx, in_flight);
        advance_orchestrator(
            engine,
            memory,
            clients,
            poll,
            scheduler,
            orchestrator,
            in_flight,
            pending_kills,
            cmd_tx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::syscalls::scan_syscall_buffer;

    #[test]
    fn scan_finds_complete_command() {
        let mut buf = "some text [[PYTHON: print('hello')]] more text".to_string();
        let result = scan_syscall_buffer(&mut buf);
        assert_eq!(result, Some("[[PYTHON: print('hello')]]".to_string()));
        assert!(buf.is_empty());
    }

    #[test]
    fn scan_returns_none_for_incomplete() {
        let mut buf = "some text [[ no closing bracket".to_string();
        let result = scan_syscall_buffer(&mut buf);
        assert!(result.is_none());
        assert!(!buf.is_empty());
    }

    #[test]
    fn scan_clears_on_overflow() {
        let mut buf = "x".repeat(8001);
        let result = scan_syscall_buffer(&mut buf);
        assert!(result.is_none());
        assert!(buf.is_empty());
    }

    #[test]
    fn scan_empty_buffer_returns_none() {
        let mut buf = String::new();
        let result = scan_syscall_buffer(&mut buf);
        assert!(result.is_none());
    }

    #[test]
    fn scan_only_opening_brackets() {
        let mut buf = "[[start but never ends".to_string();
        let result = scan_syscall_buffer(&mut buf);
        assert!(result.is_none());
        assert!(!buf.is_empty());
    }
}
