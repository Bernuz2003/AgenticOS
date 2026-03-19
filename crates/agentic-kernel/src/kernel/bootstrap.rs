use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use std::time::Instant;

use crate::commands::MetricsState;
use crate::config;
use crate::inference_worker::{self, InferenceCmd, InferenceResult};
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::resource_governor::ResourceGovernor;
use crate::runtime::syscalls::{self, SyscallCmd, SyscallCompletion};
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::tools::SyscallRateMap;
use crate::transport::Client;

use super::{
    recovery,
    server::{Kernel, SERVER, WORKER_WAKE_TOKEN},
};

/// Costruisce e inizializza l'istanza principale del Kernel di AgenticOS.
///
/// Questa funzione si occupa di allocare e collegare tutti i sottosistemi principali:
/// rete (mio), memoria, database SQLite, worker thread asincroni e i vari registry.
pub(crate) fn build_kernel(config: &config::KernelConfig) -> io::Result<Kernel> {
    // 1. Configurazione del loop di eventi di rete (mio)
    let poll = Poll::new()?;
    let events = Events::with_capacity(128);
    let addr: std::net::SocketAddr = format!("{}:{}", config.network.host, config.network.port)
        .parse()
        .expect("valid listen address");
    let mut server = TcpListener::bind(addr)?;
    poll.registry()
        .register(&mut server, SERVER, Interest::READABLE)?;
    let worker_waker = Arc::new(mio::Waker::new(poll.registry(), WORKER_WAKE_TOKEN)?);

    // 2. Inizializzazione della memoria e del catalogo dei modelli
    let memory = build_memory(config)?;
    let shutdown_requested: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let model_catalog =
        ModelCatalog::discover(config.paths.models_dir.clone()).map_err(io::Error::other)?;
    let checkpoint_interval_secs = config.checkpoint.interval_secs;

    // 3. Avvio del thread dedicato all'inferenza LLM
    let (cmd_tx, cmd_rx) = mpsc::channel::<InferenceCmd>();
    let (result_tx, result_rx) = mpsc::channel::<InferenceResult>();
    let worker_handle =
        inference_worker::spawn_worker(result_tx, cmd_rx, Some(worker_waker.clone()));

    // 4. Avvio del thread dedicato all'esecuzione delle syscall (tool)
    let syscall_rates = Arc::new(std::sync::Mutex::new(SyscallRateMap::new()));
    let (syscall_cmd_tx, syscall_cmd_rx) = mpsc::channel::<SyscallCmd>();
    let (syscall_result_tx, syscall_result_rx) = mpsc::channel::<SyscallCompletion>();
    let syscall_worker_handle = syscalls::spawn_syscall_worker(
        Arc::clone(&syscall_rates),
        syscall_result_tx,
        syscall_cmd_rx,
        Some(worker_waker),
    );

    // 5. Setup dello storage SQLite, record di boot e procedure di recovery
    let mut storage =
        StorageService::open(&config.paths.database_path).map_err(io::Error::other)?;
    let boot_record = storage
        .record_kernel_boot(env!("CARGO_PKG_VERSION"))
        .map_err(io::Error::other)?;
    let legacy_import = storage
        .import_legacy_timelines_once(&config::repository_path("timeline_sessions"))
        .map_err(io::Error::other)?;
    let recovery_report = recovery::run_boot_recovery(&mut storage)?;
    let runtime_registry = RuntimeRegistry::load(&mut storage).map_err(io::Error::other)?;
    let resource_governor =
        ResourceGovernor::load(&mut storage, config.resources.clone()).map_err(io::Error::other)?;
    let session_registry =
        SessionRegistry::load(&mut storage, boot_record.boot_id).map_err(io::Error::other)?;

    // 6. Generazione del token di autenticazione per la sessione corrente
    let auth_disabled = config.auth.disabled;
    let auth_token = write_auth_token(config)?;

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        %addr,
        memory_swap_async = config.memory.swap_async,
        swap_dir = %config.memory.swap_dir.display(),
        database_path = %config.paths.database_path.display(),
        database_boot_id = boot_record.boot_id,
        imported_legacy_sessions = legacy_import.imported_sessions,
        imported_legacy_turns = legacy_import.imported_turns,
        imported_legacy_messages = legacy_import.imported_messages,
        recovery_reset_sessions = recovery_report.stale_active_sessions_reset,
        recovery_interrupted_runs = recovery_report.interrupted_process_runs,
        recovery_interrupted_turns = recovery_report.interrupted_turns,
        recovery_logical_resume_sessions = recovery_report.logical_resume_sessions,
        recovery_strong_restore_candidates = recovery_report.strong_restore_candidate_sessions,
        recovery_pending_runtime_queue = recovery_report.pending_runtime_queue_entries,
        resource_ram_budget_bytes = config.resources.ram_budget_bytes,
        resource_vram_budget_bytes = config.resources.vram_budget_bytes,
        persisted_runtimes = runtime_registry.runtime_count(),
        persisted_sessions = session_registry.session_count(),
        checkpoint_interval_secs,
        auth_disabled,
        "AgenticOS Kernel ready"
    );

    // 7. Assemblaggio finale della struct Kernel
    Ok(Kernel {
        poll,
        events,
        server,
        clients: HashMap::<Token, Client>::new(),
        unique_token: Token(WORKER_WAKE_TOKEN.0 + 1),
        log_connections: config.network.log_connections,
        memory,
        runtime_registry,
        resource_governor,
        shutdown_requested,
        model_catalog,
        scheduler: ProcessScheduler::new(),
        orchestrator: Orchestrator::new(),
        remote_deadline_timeout: std::time::Duration::from_millis(
            config
                .openai_responses
                .timeout_ms
                .max(config.groq_responses.timeout_ms)
                .max(config.openrouter.timeout_ms)
                .max(1),
        ),
        syscall_deadline_timeout: std::time::Duration::from_secs(config.tools.timeout_s.max(1)),
        checkpoint_interval_secs,
        last_checkpoint: Instant::now(),
        cmd_tx,
        result_rx,
        syscall_cmd_tx,
        syscall_result_rx,
        in_flight: HashSet::new(),
        pending_kills: Vec::new(),
        pending_events: Vec::new(),
        syscall_wait_since: HashMap::new(),
        remote_timeout_reported: HashSet::new(),
        next_event_sequence: 0,
        worker_handle: Some(worker_handle),
        syscall_worker_handle: Some(syscall_worker_handle),
        metrics: MetricsState::new(),
        tool_registry: ToolRegistry::with_builtins(),
        auth_token,
        auth_disabled,
        session_registry,
        storage,
    })
}

/// Inizializza il sottosistema NeuralMemory.
///
/// Applica le quote slot e configura l'eventuale worker asincrono per lo swap su disco.
fn build_memory(config: &config::KernelConfig) -> io::Result<NeuralMemory> {
    let mut memory = NeuralMemory::new().map_err(|e| io::Error::other(e.to_string()))?;
    memory.set_token_slot_quota_per_pid(config.memory.token_slot_quota_per_pid);
    if let Err(e) = memory.configure_async_swap(
        config.memory.swap_async,
        Some(config.memory.swap_dir.clone()),
    ) {
        tracing::error!(%e, "Failed to configure async swap worker");
    }
    Ok(memory)
}

/// Genera un token randomico (32 byte), lo formatta in esadecimale e lo salva su disco.
///
/// Questo token viene utilizzato dai client (come la GUI Tauri) per autenticarsi via TCP.
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
