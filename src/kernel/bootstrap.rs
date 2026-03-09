use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use std::time::Instant;

use crate::commands::MetricsState;
use crate::config;
use crate::engine::LLMEngine;
use crate::inference_worker::{self, InferenceCmd, InferenceResult};
use crate::memory::{MemoryConfig, NeuralMemory};
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::scheduler::ProcessScheduler;
use crate::tools::SyscallRateMap;
use crate::transport::Client;

use super::server::{Kernel, SERVER};

pub(crate) fn build_kernel(config: &config::KernelConfig) -> io::Result<Kernel> {
    let poll = Poll::new()?;
    let events = Events::with_capacity(128);
    let addr: std::net::SocketAddr = format!("{}:{}", config.network.host, config.network.port)
        .parse()
        .expect("valid listen address");
    let mut server = TcpListener::bind(addr)?;
    poll.registry()
        .register(&mut server, SERVER, Interest::READABLE)?;

    let memory = build_memory(config)?;
    let shutdown_requested: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let model_catalog = ModelCatalog::discover(config.paths.models_dir.clone())
        .map_err(io::Error::other)?;
    let checkpoint_interval_secs = config.checkpoint.interval_secs;

    let (cmd_tx, cmd_rx) = mpsc::channel::<InferenceCmd>();
    let (result_tx, result_rx) = mpsc::channel::<InferenceResult>();
    let worker_handle = inference_worker::spawn_worker(result_tx, cmd_rx);

    let auth_disabled = config.auth.disabled;
    let auth_token = write_auth_token(config)?;

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        %addr,
        memory_active = config.memory.active,
        memory_swap_async = config.memory.swap_async,
        swap_dir = %config.memory.swap_dir.display(),
        checkpoint_interval_secs,
        auth_disabled,
        "AgenticOS Kernel ready"
    );

    Ok(Kernel {
        poll,
        events,
        server,
        clients: HashMap::<Token, Client>::new(),
        unique_token: Token(SERVER.0 + 1),
        log_connections: config.network.log_connections,
        memory,
        engine_state: Option::<LLMEngine>::None,
        shutdown_requested,
        model_catalog,
        scheduler: ProcessScheduler::new(),
        orchestrator: Orchestrator::new(),
        poll_timeout_ms: config.network.poll_timeout_ms,
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

fn build_memory(config: &config::KernelConfig) -> io::Result<NeuralMemory> {
    let mem_config = MemoryConfig {
        block_size: config.memory.block_size,
        hidden_dim: config.memory.hidden_dim,
        total_memory_mb: config.memory.total_memory_mb,
    };
    let mut memory = NeuralMemory::new(mem_config).map_err(|e| io::Error::other(e.to_string()))?;
    memory.set_active(config.memory.active);
    memory.set_token_slot_quota_per_pid(config.memory.token_slot_quota_per_pid);
    if let Err(e) = memory.configure_async_swap(
        config.memory.swap_async,
        Some(config.memory.swap_dir.clone()),
    ) {
        tracing::error!(%e, "Failed to configure async swap worker");
    }
    Ok(memory)
}

fn write_auth_token(config: &config::KernelConfig) -> io::Result<String> {
    use std::io::Write as _;

    let mut buf = [0u8; 32];
    getrandom::getrandom(&mut buf)
        .map_err(|e| io::Error::other(format!("failed to generate auth token: {}", e)))?;

    let hex: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    let token_path = config.paths.kernel_token_path.as_path();
    if let Some(parent) = token_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(token_path)?;
    file.write_all(hex.as_bytes())?;
    Ok(hex)
}