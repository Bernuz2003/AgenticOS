use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::backend::shutdown_managed_runtimes;
use crate::checkpoint;
use crate::commands::MetricsState;
use crate::config;
use crate::engine::LLMEngine;
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
use crate::runtime::TurnAssemblyStore;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::services::job_scheduler::{JobScheduler, SCHEDULER_SYSTEM_OWNER_ID};
use crate::services::orchestration_runtime::{start_orchestration, OrchestrationStartError};
use crate::services::process_runtime::kill_managed_process_with_session;
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::transport::{
    handle_read_with_registry, handle_write, needs_writable_interest, writable_interest, Client,
};

use super::bootstrap;
use super::shutdown::shutdown_workers;
use super::wakers::{
    classify_wake_reason, instant_for_timestamp, refresh_syscall_wait_tracking, LoopWakeReason,
    SERVER, WORKER_WAKE_TOKEN,
};

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
    pub(crate) job_scheduler: JobScheduler,
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
    pub(crate) turn_assembly: TurnAssemblyStore,
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
                shutdown_managed_runtimes();
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
                &mut self.turn_assembly,
                &mut self.in_flight,
                &mut self.pending_kills,
                &mut self.pending_events,
                &self.tool_registry,
            );
            self.reconcile_scheduled_job_runs();

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
                &mut self.session_registry,
                &mut self.storage,
                &mut self.turn_assembly,
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
        let now_ms = crate::storage::current_timestamp_ms();

        if self.checkpoint_interval_secs > 0 {
            candidates.push(DeadlineCandidate {
                reason: DeadlineReason::Checkpoint,
                at: self.last_checkpoint + Duration::from_secs(self.checkpoint_interval_secs),
                subject_id: None,
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
                subject_id: Some(pid),
            });
        }

        for (pid, started_at) in &self.syscall_wait_since {
            candidates.push(DeadlineCandidate {
                reason: DeadlineReason::SyscallTimeout,
                at: *started_at + self.syscall_deadline_timeout,
                subject_id: Some(*pid),
            });
        }

        if let Some(next_run_at_ms) = self.job_scheduler.next_due_at_ms() {
            candidates.push(DeadlineCandidate {
                reason: DeadlineReason::ScheduledJob,
                at: instant_for_timestamp(now, now_ms, next_run_at_ms),
                subject_id: None,
            });
        }

        if let Some((_, timeout_at_ms)) = self.job_scheduler.next_timeout_at_ms() {
            candidates.push(DeadlineCandidate {
                reason: DeadlineReason::ScheduledJobTimeout,
                at: instant_for_timestamp(now, now_ms, timeout_at_ms),
                subject_id: None,
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
                run_auto_checkpoint(
                    self.runtime_registry.current_engine(),
                    &self.model_catalog,
                    &self.scheduler,
                    &self.metrics,
                    &self.memory,
                );
            }
            DeadlineReason::RemoteTimeout => {
                if let Some(pid) = deadline.subject_id {
                    self.remote_timeout_reported.insert(pid);
                    tracing::warn!(
                        pid,
                        "KERNEL_DEADLINE: remote response timeout elapsed while waiting for worker result"
                    );
                }
            }
            DeadlineReason::SyscallTimeout => {
                if let Some(pid) = deadline.subject_id {
                    self.enforce_syscall_timeout(pid);
                }
            }
            DeadlineReason::ScheduledJob => self.dispatch_due_scheduled_jobs(),
            DeadlineReason::ScheduledJobTimeout => self.enforce_scheduled_job_timeouts(),
        }
    }

    fn dispatch_due_scheduled_jobs(&mut self) {
        let due_job_ids = self
            .job_scheduler
            .due_job_ids(crate::storage::current_timestamp_ms());
        for job_id in due_job_ids {
            let plan = match self.job_scheduler.dispatch_plan(job_id) {
                Ok(plan) => plan,
                Err(err) => {
                    tracing::warn!(job_id, %err, "SCHEDULER: failed to build dispatch plan");
                    continue;
                }
            };

            match start_orchestration(
                &mut self.runtime_registry,
                &mut self.resource_governor,
                &mut self.memory,
                &mut self.model_catalog,
                &mut self.scheduler,
                &mut self.orchestrator,
                &mut self.session_registry,
                &mut self.storage,
                &mut self.pending_events,
                &self.tool_registry,
                SCHEDULER_SYSTEM_OWNER_ID,
                plan.workflow,
            ) {
                Ok(started) => {
                    for _ in 0..started.spawned {
                        self.metrics.inc_exec_started();
                    }
                    if let Err(err) = self.job_scheduler.mark_started(
                        &mut self.storage,
                        plan.job_id,
                        plan.trigger_at_ms,
                        plan.attempt,
                        started.orchestration_id,
                    ) {
                        tracing::error!(
                            job_id = plan.job_id,
                            orchestration_id = started.orchestration_id,
                            %err,
                            "SCHEDULER: failed to persist running job state"
                        );
                    }
                    self.pending_events
                        .push(agentic_control_models::KernelEvent::LobbyChanged {
                            reason: "scheduled_job_started".to_string(),
                        });
                }
                Err(err) => {
                    let detail = match err {
                        OrchestrationStartError::NoModelLoaded => "no_model_loaded".to_string(),
                        OrchestrationStartError::InvalidGraph(inner) => inner.to_string(),
                        OrchestrationStartError::RoutingFailed(inner) => inner,
                    };
                    if let Err(persist_err) = self.job_scheduler.mark_dispatch_failed(
                        &mut self.storage,
                        plan.job_id,
                        plan.trigger_at_ms,
                        plan.attempt,
                        &detail,
                    ) {
                        tracing::error!(
                            job_id = plan.job_id,
                            %persist_err,
                            "SCHEDULER: failed to persist dispatch failure"
                        );
                    }
                    self.pending_events
                        .push(agentic_control_models::KernelEvent::LobbyChanged {
                            reason: "scheduled_job_failed".to_string(),
                        });
                }
            }
        }
    }

    fn reconcile_scheduled_job_runs(&mut self) {
        let orch_ids = self.job_scheduler.orchestration_ids();
        for orch_id in orch_ids {
            let Some(orchestration) = self.orchestrator.get(orch_id) else {
                continue;
            };
            if !orchestration.is_finished() {
                continue;
            }
            let (_, _, _, failed, _) = orchestration.counts();
            let result = if failed > 0 {
                self.job_scheduler.complete_orchestration(
                    &mut self.storage,
                    orch_id,
                    "failed",
                    Some("workflow_failed"),
                )
            } else {
                self.job_scheduler.complete_orchestration(
                    &mut self.storage,
                    orch_id,
                    "completed",
                    None,
                )
            };
            if let Err(err) = result {
                tracing::error!(orch_id, %err, "SCHEDULER: failed to finalize job run");
                continue;
            }
            self.pending_events
                .push(agentic_control_models::KernelEvent::LobbyChanged {
                    reason: "scheduled_job_completed".to_string(),
                });
        }
    }

    fn enforce_scheduled_job_timeouts(&mut self) {
        let timed_out_job_ids = self
            .job_scheduler
            .timeout_job_ids(crate::storage::current_timestamp_ms());

        for job_id in timed_out_job_ids {
            let orchestration_id =
                match self.job_scheduler.mark_timed_out(&mut self.storage, job_id) {
                    Ok(value) => value,
                    Err(err) => {
                        tracing::error!(job_id, %err, "SCHEDULER: failed to mark timeout");
                        continue;
                    }
                };

            if let Some(orch_id) = orchestration_id {
                let running_pids = self
                    .orchestrator
                    .get(orch_id)
                    .map(|orch| orch.running_pids())
                    .unwrap_or_default();
                for pid in running_pids {
                    let Some(runtime_id) = self
                        .runtime_registry
                        .runtime_id_for_pid(pid)
                        .map(ToString::to_string)
                    else {
                        continue;
                    };
                    let Some(engine) = self.runtime_registry.engine_mut(&runtime_id) else {
                        continue;
                    };
                    kill_managed_process_with_session(
                        engine,
                        &mut self.memory,
                        &mut self.scheduler,
                        &mut self.session_registry,
                        &mut self.storage,
                        pid,
                        "scheduled_job_timeout",
                    );
                    if let Err(err) = self.runtime_registry.release_pid(&mut self.storage, pid) {
                        tracing::warn!(
                            pid,
                            %err,
                            "SCHEDULER: failed to release pid after job timeout"
                        );
                    }
                    self.pending_events.push(
                        agentic_control_models::KernelEvent::SessionFinished {
                            pid,
                            tokens_generated: None,
                            elapsed_secs: None,
                            reason: "scheduled_job_timeout".to_string(),
                        },
                    );
                    self.pending_events.push(
                        agentic_control_models::KernelEvent::WorkspaceChanged {
                            pid,
                            reason: "scheduled_job_timeout".to_string(),
                        },
                    );
                }
            }

            self.pending_events
                .push(agentic_control_models::KernelEvent::LobbyChanged {
                    reason: "scheduled_job_timeout".to_string(),
                });
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

fn run_auto_checkpoint(
    engine_state: Option<&LLMEngine>,
    model_catalog: &ModelCatalog,
    scheduler: &ProcessScheduler,
    metrics: &MetricsState,
    memory: &NeuralMemory,
) {
    let path = checkpoint::default_checkpoint_path();
    let snap =
        checkpoint::build_kernel_snapshot(engine_state, model_catalog, scheduler, metrics, memory);

    match checkpoint::save_checkpoint(&snap, &path) {
        Ok(msg) => tracing::debug!(msg, "auto-checkpoint"),
        Err(err) => tracing::warn!(%err, "auto-checkpoint failed"),
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
                &mut kernel.job_scheduler,
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
                &mut kernel.turn_assembly,
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
