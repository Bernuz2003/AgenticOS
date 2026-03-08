mod backend;
mod checkpoint;
mod config;
mod commands;
mod engine;
mod errors;
mod inference_worker;
mod memory;
mod model_catalog;
mod orchestrator;
mod process;
mod prompting;
mod protocol;
mod runtime;
mod scheduler;
mod tools;
mod transport;

use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Instant;

use commands::MetricsState;
use config::env_bool;
use engine::LLMEngine;
use inference_worker::{InferenceCmd, InferenceResult};
use memory::{MemoryConfig, NeuralMemory};
use model_catalog::ModelCatalog;
use orchestrator::Orchestrator;
use runtime::run_engine_tick;
use scheduler::ProcessScheduler;
use tools::SyscallRateMap;
use transport::{handle_read, handle_write, needs_writable_interest, writable_interest, Client};

const SERVER: Token = Token(0);

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
struct Kernel {
    poll: Poll,
    events: Events,
    server: TcpListener,
    clients: HashMap<Token, Client>,
    unique_token: Token,
    log_connections: bool,
    memory: NeuralMemory,
    engine_state: Option<LLMEngine>,
    shutdown_requested: Arc<AtomicBool>,
    model_catalog: ModelCatalog,
    scheduler: ProcessScheduler,
    orchestrator: Orchestrator,
    checkpoint_interval_secs: u64,
    last_checkpoint: Instant,
    // ── Inference worker (checkout/checkin) ──────────────────────
    cmd_tx: mpsc::Sender<InferenceCmd>,
    result_rx: mpsc::Receiver<InferenceResult>,
    in_flight: HashSet<u64>,
    pending_kills: Vec<u64>,
    worker_handle: Option<JoinHandle<()>>,
    // ── Metrics & rate-limiting (C6 — no global statics) ────────
    metrics: MetricsState,
    syscall_rates: SyscallRateMap,
    // ── Auth (C3) ───────────────────────────────────────────────
    auth_token: String,
    auth_disabled: bool,
}

impl Kernel {
    fn new() -> io::Result<Self> {
        let poll = Poll::new()?;
        let events = Events::with_capacity(128);
        let port = config::env_u64("AGENTIC_PORT", 6380) as u16;
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port)
            .parse()
            .expect("valid listen address");
        let mut server = TcpListener::bind(addr)?;
        poll.registry()
            .register(&mut server, SERVER, Interest::READABLE)?;

        let log_connections = env_bool("AGENTIC_LOG_CONNECTIONS", false);

        let mem_config = MemoryConfig {
            block_size: 16,
            hidden_dim: 256,
            total_memory_mb: 64,
        };
        let memory = NeuralMemory::new(mem_config)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let memory_active = env_bool("AGENTIC_MEMORY_ACTIVE", true);
        let memory_swap_async = env_bool("AGENTIC_MEMORY_SWAP_ASYNC", true);
        let memory_swap_dir = std::env::var("AGENTIC_MEMORY_SWAP_DIR").ok();
        // Note: set_active and configure_async_swap require &mut to the owned NeuralMemory
        // which is available here since we still own `memory` directly.
        let mut memory = memory;
        memory.set_active(memory_active);
        if let Err(e) = memory.configure_async_swap(
            memory_swap_async,
            memory_swap_dir
                .as_ref()
                .map(|v| std::path::PathBuf::from(v.as_str())),
        ) {
            tracing::error!(%e, "Failed to configure async swap worker");
        }

        let engine_state: Option<LLMEngine> = None;
        let shutdown_requested: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
        let model_catalog = ModelCatalog::discover("models")
            .map_err(io::Error::other)?;
        let scheduler = ProcessScheduler::new();

        let checkpoint_interval_secs: u64 = std::env::var("AGENTIC_CHECKPOINT_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        // ── Inference worker ────────────────────────────────────────
        let (cmd_tx, cmd_rx) = mpsc::channel::<InferenceCmd>();
        let (result_tx, result_rx) = mpsc::channel::<InferenceResult>();
        let worker_handle = inference_worker::spawn_worker(result_tx, cmd_rx);

        // ── Auth token (C3) ─────────────────────────────────────────
        let auth_disabled = env_bool("AGENTIC_AUTH_DISABLED", false);
        let auth_token = {
            use std::io::Write as _;
            let mut buf = [0u8; 32];
            getrandom::getrandom(&mut buf).expect("failed to generate auth token");
            let hex: String = buf.iter().map(|b| format!("{b:02x}")).collect();
            let token_path = std::path::Path::new("workspace/.kernel_token");
            if let Some(parent) = token_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let mut f = std::fs::File::create(token_path)
                .expect("failed to write .kernel_token");
            f.write_all(hex.as_bytes())
                .expect("failed to write .kernel_token");
            hex
        };

        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            %addr,
            memory_active,
            memory_swap_async,
            swap_dir = memory_swap_dir.as_deref().unwrap_or("workspace/swap"),
            checkpoint_interval_secs,
            auth_disabled,
            "AgenticOS Kernel ready"
        );

        Ok(Kernel {
            poll,
            events,
            server,
            clients: HashMap::new(),
            unique_token: Token(SERVER.0 + 1),
            log_connections,
            memory,
            engine_state,
            shutdown_requested,
            model_catalog,
            scheduler,
            orchestrator: Orchestrator::new(),
            checkpoint_interval_secs,
            last_checkpoint: Instant::now(),
            cmd_tx,
            result_rx,
            in_flight: HashSet::new(),
            pending_kills: Vec::new(),
            worker_handle: Some(worker_handle),
            metrics: MetricsState::new(),
            syscall_rates: SyscallRateMap::new(),
            auth_token,
            auth_disabled,
        })
    }

    fn run(&mut self) -> io::Result<()> {
        loop {
            if self.shutdown_requested.load(Ordering::SeqCst) {
                tracing::info!("Kernel graceful shutdown requested. Closing event loop.");
                // Signal the inference worker to stop.
                let _ = self.cmd_tx.send(InferenceCmd::Shutdown);
                if let Some(handle) = self.worker_handle.take() {
                    let _ = handle.join();
                }
                break;
            }

            self.poll
                .poll(&mut self.events, Some(std::time::Duration::from_millis(5)))?;

            for event in self.events.iter() {
                match event.token() {
                    SERVER => loop {
                        match self.server.accept() {
                            Ok((mut stream, peer_addr)) => {
                                let token = self.unique_token;
                                self.unique_token.0 += 1;
                                if self.log_connections {
                                    tracing::info!(%peer_addr, "New connection");
                                }
                                self.poll.registry().register(
                                    &mut stream,
                                    token,
                                    Interest::READABLE,
                                )?;
                                self.clients.insert(token, Client::new(stream, self.auth_disabled));
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                            Err(e) => tracing::error!(%e, "Accept error"),
                        }
                    },
                    token => {
                        if let Some(client) = self.clients.get_mut(&token) {
                            let mut should_close = false;

                            if event.is_readable()
                                && handle_read(
                                    client,
                                    &mut self.memory,
                                    &mut self.engine_state,
                                    &mut self.model_catalog,
                                    &mut self.scheduler,
                                    &mut self.orchestrator,
                                    token.0,
                                    &self.shutdown_requested,
                                    &self.in_flight,
                                    &mut self.pending_kills,
                                    &mut self.metrics,
                                    &self.auth_token,
                                )
                            {
                                should_close = true;
                            }

                            if !should_close && needs_writable_interest(client) {
                                self.poll.registry().reregister(
                                    &mut client.stream,
                                    token,
                                    writable_interest(),
                                )?;
                            }

                            if event.is_writable() {
                                if handle_write(client) {
                                    should_close = true;
                                } else if client.output_buffer.is_empty() {
                                    self.poll.registry().reregister(
                                        &mut client.stream,
                                        token,
                                        Interest::READABLE,
                                    )?;
                                }
                            }

                            if should_close {
                                self.clients.remove(&token);
                            }
                        }
                    }
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
            );

            // ── Auto-checkpoint ────────────────────────────────────────
            if self.checkpoint_interval_secs > 0
                && self.last_checkpoint.elapsed().as_secs() >= self.checkpoint_interval_secs
            {
                self.last_checkpoint = Instant::now();
                self.run_auto_checkpoint();
            }
        }

        Ok(())
    }

    /// Perform an automatic periodic checkpoint (best-effort, errors are logged).
    fn run_auto_checkpoint(&self) {
        use crate::checkpoint;

        let path = checkpoint::default_checkpoint_path();
        let (uptime_s, total_cmd, total_err, total_exec, total_signals) = self.metrics.snapshot();

        let (processes, generation) = {
            if let Some(engine) = self.engine_state.as_ref() {
                let procs: Vec<checkpoint::ProcessSnapshot> = engine
                    .processes
                    .iter()
                    .map(|(pid, p)| checkpoint::ProcessSnapshot {
                        pid: *pid,
                        owner_id: p.owner_id,
                        state: format!("{:?}", p.state),
                        token_count: p.tokens.len(),
                        max_tokens: p.max_tokens,
                    })
                    .collect();
                let cfg = engine.generation_config();
                let gen = Some(checkpoint::GenerationSnapshot {
                    temperature: cfg.temperature,
                    top_p: cfg.top_p,
                    seed: cfg.seed,
                    max_tokens: cfg.max_tokens,
                });
                (procs, gen)
            } else {
                (vec![], None)
            }
        };

        let active_family = self
            .engine_state
            .as_ref()
            .map(|engine| format!("{:?}", engine.loaded_family()))
            .or_else(|| {
                self.model_catalog
                    .selected_entry()
                    .map(|entry| format!("{:?}", entry.family))
            })
            .unwrap_or_else(|| "Unknown".to_string());

        let snap = checkpoint::KernelSnapshot {
            timestamp: checkpoint::now_timestamp(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            active_family,
            selected_model: self.model_catalog.selected_id.clone(),
            generation,
            processes,
            scheduler: checkpoint::snapshot_scheduler(&self.scheduler),
            metrics: checkpoint::MetricsSnapshot {
                uptime_secs: uptime_s,
                total_commands: total_cmd,
                total_errors: total_err,
                total_exec_started: total_exec,
                total_signals,
            },
            memory: checkpoint::snapshot_memory(&self.memory),
        };

        match checkpoint::save_checkpoint(&snap, &path) {
            Ok(msg) => tracing::debug!(msg, "auto-checkpoint"),
            Err(e) => tracing::warn!(%e, "auto-checkpoint failed"),
        }
    }
}

fn main() -> io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut kernel = Kernel::new()?;
    tools::cleanup_stale_temp_scripts();
    kernel.run()
}
