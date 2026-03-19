use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::commands::MetricsState;
use crate::config;
use crate::events::flush_pending_events;
use crate::inference_worker::{InferenceCmd, InferenceResult};
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::resource_governor::ResourceGovernor;
use crate::runtime::deadlines::{
    compute_poll_timeout, pick_next_deadline, DeadlineCandidate, DeadlineReason, NextDeadline,
};
use crate::runtime::run_engine_tick;
use crate::runtime::syscalls::{SyscallCmd, SyscallCompletion};
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::services::process_runtime::kill_managed_process_with_session;
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::transport::{
    handle_read_with_registry, handle_write, needs_writable_interest, writable_interest, Client,
};

use super::{bootstrap, checkpointing};

pub(crate) const SERVER: Token = Token(0);
pub(crate) const WORKER_WAKE_TOKEN: Token = Token(1);

#[derive(Debug, Clone, Copy)]
enum LoopWakeReason {
    Network,
    Worker,
    Deadline(DeadlineReason),
    SpuriousWake,
}

impl LoopWakeReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Worker => "worker",
            Self::Deadline(reason) => reason.as_str(),
            Self::SpuriousWake => "spurious_wake",
        }
    }
}

/// Encapsulates all kernel state into a single structure.
///
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
    pub(crate) runtime_registry: RuntimeRegistry,
    pub(crate) resource_governor: ResourceGovernor,
    pub(crate) shutdown_requested: Arc<AtomicBool>,
    pub(crate) model_catalog: ModelCatalog,
    pub(crate) scheduler: ProcessScheduler,
    pub(crate) orchestrator: Orchestrator,
    pub(crate) remote_deadline_timeout: Duration,
    pub(crate) syscall_deadline_timeout: Duration,
    pub(crate) checkpoint_interval_secs: u64,
    pub(crate) last_checkpoint: Instant,
    pub(crate) cmd_tx: mpsc::Sender<InferenceCmd>,
    pub(crate) result_rx: mpsc::Receiver<InferenceResult>,
    pub(crate) syscall_cmd_tx: mpsc::Sender<SyscallCmd>,
    pub(crate) syscall_result_rx: mpsc::Receiver<SyscallCompletion>,
    pub(crate) in_flight: HashSet<u64>,
    pub(crate) pending_kills: Vec<u64>,
    pub(crate) pending_events: Vec<agentic_control_models::KernelEvent>,
    pub(crate) syscall_wait_since: HashMap<u64, Instant>,
    pub(crate) remote_timeout_reported: HashSet<u64>,
    pub(crate) next_event_sequence: u64,
    pub(crate) worker_handle: Option<JoinHandle<()>>,
    pub(crate) syscall_worker_handle: Option<JoinHandle<()>>,
    pub(crate) metrics: MetricsState,
    pub(crate) tool_registry: ToolRegistry,
    pub(crate) auth_token: String,
    pub(crate) auth_disabled: bool,
    pub(crate) session_registry: SessionRegistry,
    pub(crate) storage: StorageService,
}

impl Kernel {
    pub(crate) fn new(config: &config::KernelConfig) -> io::Result<Self> {
        bootstrap::build_kernel(config)
    }

    /// Avvia il loop di eventi principale del Kernel.
    ///
    /// Questo loop (single-thread, basato su `mio`) è il cuore del sistema e gestisce:
    /// 1. Il polling di I/O non bloccante (network e wake dai worker thread).
    /// 2. L'accettazione e la gestione delle connessioni client.
    /// 3. Il "tick" del motore di inferenza (LLMEngine e risorse).
    /// 4. I timeout, i flush di rete e il salvataggio periodico dello stato (checkpoint).
    pub(crate) fn run(&mut self) -> io::Result<()> {
        loop {
            // 1. Controllo dello spegnimento graceful
            if self.shutdown_requested.load(Ordering::SeqCst) {
                tracing::info!("Kernel graceful shutdown requested. Closing event loop.");
                shutdown_workers(
                    &self.cmd_tx,
                    &self.syscall_cmd_tx,
                    &mut self.worker_handle,
                    &mut self.syscall_worker_handle,
                );
                break;
            }

            // 2. Aggiornamento timer per l'intercettazione dei timeout delle syscall
            refresh_syscall_wait_tracking(
                &self.runtime_registry,
                &mut self.syscall_wait_since,
                Instant::now(),
            );

            let now = Instant::now();
            let next_deadline = self.next_deadline(now);
            let poll_timeout = compute_poll_timeout(now, next_deadline);

            // 3. Blocco sul polling degli eventi (con timeout calcolato)
            self.poll.poll(&mut self.events, poll_timeout)?;

            let event_batch: Vec<(Token, bool, bool)> = self
                .events
                .iter()
                .map(|event| (event.token(), event.is_readable(), event.is_writable()))
                .collect();

            let had_network_events = event_batch
                .iter()
                .any(|(token, _, _)| *token != WORKER_WAKE_TOKEN);

            // 4. Gestione I/O di rete: Accettazione, Lettura e Scrittura
            for (token, readable, writable) in event_batch {
                match token {
                    SERVER => accept_pending_clients(self)?,
                    WORKER_WAKE_TOKEN => {}
                    token => handle_client_event(self, token, readable, writable)?,
                }
            }

            // 5. Avanzamento dell'engine LLM, schedulatore e orchestratore
            let tick_report = run_engine_tick(
                &mut self.runtime_registry,
                &mut self.resource_governor,
                &mut self.memory,
                &mut self.model_catalog,
                &mut self.clients,
                &self.poll,
                &mut self.scheduler,
                &mut self.orchestrator,
                &self.cmd_tx,
                &self.result_rx,
                &self.syscall_cmd_tx,
                &self.syscall_result_rx,
                &mut self.session_registry,
                &mut self.storage,
                &mut self.in_flight,
                &mut self.pending_kills,
                &mut self.pending_events,
                &self.tool_registry,
            );

            let wake_reason = classify_wake_reason(
                had_network_events,
                tick_report.woke_from_worker_activity(),
                next_deadline,
                Instant::now(),
            );
            if let Some(deadline) = next_deadline {
                if matches!(wake_reason, LoopWakeReason::Deadline(_)) {
                    self.handle_elapsed_deadline(deadline, Instant::now());
                }
            }
            tracing::debug!(
                reason = wake_reason.as_str(),
                swap_events = tick_report.swap_events,
                syscall_results = tick_report.syscall_results,
                worker_results = tick_report.worker_results,
                finished_processes = tick_report.finished_processes,
                checked_out_processes = tick_report.checked_out_processes,
                "KERNEL_LOOP_WAKE"
            );

            self.prune_remote_timeout_reports();
            self.drain_pending_diagnostics();

            // 6. Invio eventi asincroni accumulati ai client iscritti (es. GUI)
            flush_pending_events(
                &mut self.clients,
                &self.poll,
                &mut self.next_event_sequence,
                &mut self.storage,
                &mut self.pending_events,
            );
        }

        Ok(())
    }

    /// Calcola la prossima scadenza temporale (deadline) per il risveglio del loop.
    ///
    /// Aggrega e sceglie la più vicina tra le seguenti:
    /// - Checkpoint periodico.
    /// - Timeout di richieste LLM remote (es. API OpenAI / Groq).
    /// - Timeout delle syscall in esecuzione.
    /// - Manutenzione e backoff programmato.
    fn next_deadline(&self, now: Instant) -> Option<NextDeadline> {
        let mut candidates = Vec::new();

        if self.checkpoint_interval_secs > 0 {
            candidates.push(DeadlineCandidate {
                reason: DeadlineReason::Checkpoint,
                at: self.last_checkpoint + Duration::from_secs(self.checkpoint_interval_secs),
                subject_pid: None,
            });
        }

        for (pid, checked_out) in self.scheduler.checked_out_snapshots() {
            if checked_out.state != "AwaitingRemoteResponse" {
                continue;
            }
            if self.remote_timeout_reported.contains(&pid) {
                continue;
            }
            let timeout = checked_out
                .backend_id
                .as_deref()
                .and_then(crate::backend::remote_runtime_config_for_backend)
                .map(|cfg| Duration::from_millis(cfg.timeout_ms.max(1)))
                .unwrap_or(self.remote_deadline_timeout);
            candidates.push(DeadlineCandidate {
                reason: DeadlineReason::RemoteTimeout,
                at: checked_out.checked_out_at + timeout,
                subject_pid: Some(pid),
            });
        }

        for (pid, started_at) in &self.syscall_wait_since {
            candidates.push(DeadlineCandidate {
                reason: DeadlineReason::SyscallTimeout,
                at: *started_at + self.syscall_deadline_timeout,
                subject_pid: Some(*pid),
            });
        }

        let next = pick_next_deadline(&candidates);
        if next.is_none() {
            tracing::trace!("KERNEL_DEADLINE: no candidate; waiting for real event");
        } else if let Some(deadline) = next {
            tracing::trace!(
                reason = deadline.reason.as_str(),
                ms_until = deadline.at.saturating_duration_since(now).as_millis() as u64,
                "KERNEL_DEADLINE: next"
            );
        }
        next
    }

    fn handle_elapsed_deadline(&mut self, deadline: NextDeadline, now: Instant) {
        match deadline.reason {
            DeadlineReason::Checkpoint => {
                self.last_checkpoint = now;
                checkpointing::run_auto_checkpoint(
                    self.runtime_registry.current_engine(),
                    &self.model_catalog,
                    &self.scheduler,
                    &self.metrics,
                    &self.memory,
                );
            }
            DeadlineReason::RemoteTimeout => {
                if let Some(pid) = deadline.subject_pid {
                    self.remote_timeout_reported.insert(pid);
                    tracing::warn!(
                        pid,
                        "KERNEL_DEADLINE: remote response timeout elapsed while waiting for worker result"
                    );
                }
            }
            DeadlineReason::SyscallTimeout => {
                if let Some(pid) = deadline.subject_pid {
                    self.enforce_syscall_timeout(pid);
                }
            }
        }
    }

    fn drain_pending_diagnostics(&mut self) {
        self.pending_events.extend(
            self.storage
                .take_live_diagnostics()
                .into_iter()
                .map(|event| agentic_control_models::KernelEvent::DiagnosticRecorded { event }),
        );
    }

    /// Interrompe forzatamente un processo se una syscall supera il tempo massimo
    /// di esecuzione consentito (configurato globalmente).
    fn enforce_syscall_timeout(&mut self, pid: u64) {
        let Some(started_at) = self.syscall_wait_since.get(&pid).copied() else {
            return;
        };
        if started_at.elapsed() < self.syscall_deadline_timeout {
            return;
        }

        let Some(runtime_id) = self
            .runtime_registry
            .runtime_id_for_pid(pid)
            .map(ToString::to_string)
        else {
            self.syscall_wait_since.remove(&pid);
            return;
        };

        let Some(engine) = self.runtime_registry.engine_mut(&runtime_id) else {
            self.syscall_wait_since.remove(&pid);
            return;
        };

        tracing::warn!(
            pid,
            timeout_ms = self.syscall_deadline_timeout.as_millis() as u64,
            "KERNEL_DEADLINE: syscall timeout exceeded, terminating process"
        );
        kill_managed_process_with_session(
            engine,
            &mut self.memory,
            &mut self.scheduler,
            &mut self.session_registry,
            &mut self.storage,
            pid,
            "syscall_timeout",
        );
        if let Err(err) = self.runtime_registry.release_pid(&mut self.storage, pid) {
            tracing::warn!(pid, %err, "KERNEL_DEADLINE: failed to release pid after syscall timeout");
        }
        self.pending_events
            .push(agentic_control_models::KernelEvent::SessionFinished {
                pid,
                tokens_generated: None,
                elapsed_secs: None,
                reason: "syscall_timeout".to_string(),
            });
        self.pending_events
            .push(agentic_control_models::KernelEvent::WorkspaceChanged {
                pid,
                reason: "syscall_timeout".to_string(),
            });
        self.pending_events
            .push(agentic_control_models::KernelEvent::LobbyChanged {
                reason: "syscall_timeout".to_string(),
            });
        self.syscall_wait_since.remove(&pid);
    }

    fn prune_remote_timeout_reports(&mut self) {
        let active_remote_waiting: HashSet<u64> = self
            .scheduler
            .checked_out_snapshots()
            .into_iter()
            .filter_map(|(pid, metadata)| {
                (metadata.state == "AwaitingRemoteResponse").then_some(pid)
            })
            .collect();
        self.remote_timeout_reported
            .retain(|pid| active_remote_waiting.contains(pid));
    }
}

fn classify_wake_reason(
    had_network_events: bool,
    had_worker_activity: bool,
    next_deadline: Option<NextDeadline>,
    now: Instant,
) -> LoopWakeReason {
    if had_network_events {
        return LoopWakeReason::Network;
    }
    if had_worker_activity {
        return LoopWakeReason::Worker;
    }
    if let Some(deadline) = next_deadline {
        if now >= deadline.at {
            return LoopWakeReason::Deadline(deadline.reason);
        }
    }
    LoopWakeReason::SpuriousWake
}

fn refresh_syscall_wait_tracking(
    runtime_registry: &RuntimeRegistry,
    syscall_wait_since: &mut HashMap<u64, Instant>,
    now: Instant,
) {
    let mut waiting_now: HashSet<u64> = HashSet::new();

    for pid in runtime_registry.all_active_pids() {
        let Some(runtime_id) = runtime_registry.runtime_id_for_pid(pid) else {
            continue;
        };
        let Some(engine) = runtime_registry.engine(runtime_id) else {
            continue;
        };
        let is_waiting = engine.processes.get(&pid).is_some_and(|process| {
            process.state == crate::process::ProcessState::WaitingForSyscall
        });
        if is_waiting {
            waiting_now.insert(pid);
            syscall_wait_since.entry(pid).or_insert(now);
        }
    }

    syscall_wait_since.retain(|pid, _| waiting_now.contains(pid));
}

fn shutdown_workers(
    cmd_tx: &mpsc::Sender<InferenceCmd>,
    syscall_cmd_tx: &mpsc::Sender<SyscallCmd>,
    worker_handle: &mut Option<JoinHandle<()>>,
    syscall_worker_handle: &mut Option<JoinHandle<()>>,
) {
    let _ = cmd_tx.send(InferenceCmd::Shutdown);
    let _ = syscall_cmd_tx.send(SyscallCmd::Shutdown);
    if let Some(handle) = worker_handle.take() {
        let _ = handle.join();
    }
    if let Some(handle) = syscall_worker_handle.take() {
        let _ = handle.join();
    }
}

/// Accetta le nuove connessioni TCP in ingresso e le registra nell'event loop per la lettura.
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

/// Gestisce gli eventi di I/O per un singolo client TCP.
///
/// Si occupa di processare la lettura dei comandi (ed eseguirli) e lo svuotamento dei buffer in scrittura.
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
                &mut kernel.runtime_registry,
                &mut kernel.resource_governor,
                &mut kernel.model_catalog,
                &mut kernel.scheduler,
                &mut kernel.orchestrator,
                &mut kernel.session_registry,
                &mut kernel.storage,
                token.0,
                &kernel.shutdown_requested,
                &kernel.in_flight,
                &mut kernel.pending_kills,
                &mut kernel.pending_events,
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
