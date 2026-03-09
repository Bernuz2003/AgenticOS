use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::commands::MetricsState;
use crate::config;
use crate::engine::LLMEngine;
use crate::inference_worker::{InferenceCmd, InferenceResult};
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::runtime::run_engine_tick;
use crate::scheduler::ProcessScheduler;
use crate::tool_registry::ToolRegistry;
use crate::tools::SyscallRateMap;
use crate::transport::{handle_read_with_registry, handle_write, needs_writable_interest, writable_interest, Client};

use super::{bootstrap, checkpointing};

pub(crate) const SERVER: Token = Token(0);

/// Encapsulates all kernel state into a single structure.
///
/// # Concurrency model (C2)
///
/// All state lives on the main (mio event-loop) thread. No `Arc<Mutex>` is
/// needed because nothing is shared across threads. The inference worker
/// receives individual `AgentProcess` values via `mpsc` channels
/// (checkout/checkin pattern) and never touches the engine or memory.
///
/// `NeuralMemory` is owned directly (no `Rc<RefCell>`) — every call site
/// receives `&mut NeuralMemory` via split borrows on `Kernel`.
pub(crate) struct Kernel {
    pub(crate) poll: Poll,
    pub(crate) events: Events,
    pub(crate) server: TcpListener,
    pub(crate) clients: HashMap<Token, Client>,
    pub(crate) unique_token: Token,
    pub(crate) log_connections: bool,
    pub(crate) memory: NeuralMemory,
    pub(crate) engine_state: Option<LLMEngine>,
    pub(crate) shutdown_requested: Arc<AtomicBool>,
    pub(crate) model_catalog: ModelCatalog,
    pub(crate) scheduler: ProcessScheduler,
    pub(crate) orchestrator: Orchestrator,
    pub(crate) poll_timeout_ms: u64,
    pub(crate) checkpoint_interval_secs: u64,
    pub(crate) last_checkpoint: Instant,
    pub(crate) cmd_tx: mpsc::Sender<InferenceCmd>,
    pub(crate) result_rx: mpsc::Receiver<InferenceResult>,
    pub(crate) in_flight: HashSet<u64>,
    pub(crate) pending_kills: Vec<u64>,
    pub(crate) worker_handle: Option<JoinHandle<()>>,
    pub(crate) metrics: MetricsState,
    pub(crate) syscall_rates: SyscallRateMap,
    pub(crate) tool_registry: ToolRegistry,
    pub(crate) auth_token: String,
    pub(crate) auth_disabled: bool,
}

impl Kernel {
    pub(crate) fn new(config: &config::KernelConfig) -> io::Result<Self> {
        bootstrap::build_kernel(config)
    }

    pub(crate) fn run(&mut self) -> io::Result<()> {
        loop {
            if self.shutdown_requested.load(Ordering::SeqCst) {
                tracing::info!("Kernel graceful shutdown requested. Closing event loop.");
                shutdown_worker(&self.cmd_tx, &mut self.worker_handle);
                break;
            }

            self.poll.poll(
                &mut self.events,
                Some(std::time::Duration::from_millis(self.poll_timeout_ms)),
            )?;

            let event_batch: Vec<(Token, bool, bool)> = self
                .events
                .iter()
                .map(|event| (event.token(), event.is_readable(), event.is_writable()))
                .collect();

            for (token, readable, writable) in event_batch {
                match token {
                    SERVER => accept_pending_clients(self)?,
                    token => handle_client_event(self, token, readable, writable)?,
                }
            }

            run_engine_tick(
                &mut self.engine_state,
                &mut self.memory,
                &mut self.clients,
                &self.poll,
                &mut self.scheduler,
                &mut self.orchestrator,
                &self.cmd_tx,
                &self.result_rx,
                &mut self.in_flight,
                &mut self.pending_kills,
                &mut self.syscall_rates,
                &self.tool_registry,
            );

            checkpointing::maybe_run_auto_checkpoint(
                self.checkpoint_interval_secs,
                &mut self.last_checkpoint,
                self.engine_state.as_ref(),
                &self.model_catalog,
                &self.scheduler,
                &self.metrics,
                &self.memory,
            );
        }

        Ok(())
    }
}

fn shutdown_worker(
    cmd_tx: &mpsc::Sender<InferenceCmd>,
    worker_handle: &mut Option<JoinHandle<()>>,
) {
    let _ = cmd_tx.send(InferenceCmd::Shutdown);
    if let Some(handle) = worker_handle.take() {
        let _ = handle.join();
    }
}

fn accept_pending_clients(kernel: &mut Kernel) -> io::Result<()> {
    loop {
        match kernel.server.accept() {
            Ok((mut stream, peer_addr)) => {
                let token = kernel.unique_token;
                kernel.unique_token.0 += 1;
                if kernel.log_connections {
                    tracing::info!(%peer_addr, "New connection");
                }
                kernel
                    .poll
                    .registry()
                    .register(&mut stream, token, Interest::READABLE)?;
                kernel
                    .clients
                    .insert(token, Client::new(stream, kernel.auth_disabled));
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) => tracing::error!(%e, "Accept error"),
        }
    }

    Ok(())
}

fn handle_client_event(
    kernel: &mut Kernel,
    token: Token,
    readable: bool,
    writable: bool,
) -> io::Result<()> {
    if let Some(client) = kernel.clients.get_mut(&token) {
        let mut should_close = false;

        if readable
            && handle_read_with_registry(
                client,
                &mut kernel.memory,
                &mut kernel.engine_state,
                &mut kernel.model_catalog,
                &mut kernel.scheduler,
                &mut kernel.orchestrator,
                token.0,
                &kernel.shutdown_requested,
                &kernel.in_flight,
                &mut kernel.pending_kills,
                &mut kernel.metrics,
                &mut kernel.tool_registry,
                &kernel.auth_token,
            )
        {
            should_close = true;
        }

        if !should_close && needs_writable_interest(client) {
            kernel
                .poll
                .registry()
                .reregister(&mut client.stream, token, writable_interest())?;
        }

        if writable {
            if handle_write(client) {
                should_close = true;
            } else if client.output_buffer.is_empty() {
                kernel
                    .poll
                    .registry()
                    .reregister(&mut client.stream, token, Interest::READABLE)?;
            }
        }

        if should_close {
            kernel.clients.remove(&token);
        }
    }

    Ok(())
}