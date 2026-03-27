//! Async swap manager for the logical residency layer.
//!
//! Owns the swap worker thread, job queue, and waiting-PID tracking.
//! Extracted from `core.rs` (M13) and now used by `LogicalResidencyManager`
//! so parking and persistence stay backend-neutral.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use super::restore;
use super::restore::PreparedSwapTarget;
use super::types::ContextSlotId;
use super::types::SlotPersistenceKind;
use super::types::SwapEvent;
use crate::backend;
use crate::errors::MemoryError;
use crate::prompting::PromptFamily;

// ── Internal message types ──────────────────────────────────────────────

pub(super) struct SwapJob {
    pub pid: u64,
    pub slot_id: ContextSlotId,
    pub backend_id: String,
    pub family: PromptFamily,
    pub target: PreparedSwapTarget,
    pub pressure_bytes: usize,
}

struct SwapResult {
    pid: u64,
    slot_id: ContextSlotId,
    success: bool,
    detail: String,
    persistence_kind: SlotPersistenceKind,
    final_path: PathBuf,
}

// ── Counters (owned by NeuralMemory, mutated via &mut references) ───────

/// Swap-specific subset of counters that `SwapManager` updates through
/// mutable references passed by `NeuralMemory`.
pub(super) struct SwapCounterDeltas {
    pub swap_count_inc: u64,
    pub swap_failures_inc: u64,
    pub swap_faults_inc: u64,
}

impl SwapCounterDeltas {
    pub fn zero() -> Self {
        Self {
            swap_count_inc: 0,
            swap_failures_inc: 0,
            swap_faults_inc: 0,
        }
    }
}

// ── SwapManager ─────────────────────────────────────────────────────────

pub(super) struct SwapManager {
    enabled: bool,
    dir: PathBuf,
    tx: Option<Sender<SwapJob>>,
    rx: Option<Receiver<SwapResult>>,
    waiting: HashSet<u64>,
    worker_crashes: u64,
}

impl SwapManager {
    pub fn new() -> Self {
        Self {
            enabled: false,
            dir: crate::config::kernel_config().memory.swap_dir.clone(),
            tx: None,
            rx: None,
            waiting: HashSet::new(),
            worker_crashes: 0,
        }
    }

    // ── Configuration ───────────────────────────────────────────────

    pub fn configure(
        &mut self,
        enabled: bool,
        swap_dir: Option<PathBuf>,
    ) -> Result<(), MemoryError> {
        if !enabled {
            self.enabled = false;
            self.tx = None;
            self.rx = None;
            self.waiting.clear();
            return Ok(());
        }

        let validated = restore::resolve_valid_swap_dir(swap_dir).map_err(MemoryError::Swap)?;
        self.dir = validated;
        self.spawn_worker()
    }

    /// Spawn (or re-spawn) the background worker thread.
    fn spawn_worker(&mut self) -> Result<(), MemoryError> {
        let (tx_job, rx_job) = mpsc::channel::<SwapJob>();
        let (tx_result, rx_result) = mpsc::channel::<SwapResult>();
        thread::Builder::new()
            .name("agentic_swap_worker".to_string())
            .spawn(move || {
                while let Ok(job) = rx_job.recv() {
                    let result = backend::persist_context_slot_payload_for_backend(
                        &job.backend_id,
                        job.family,
                        job.slot_id,
                        &job.target.final_path,
                    )
                    .map(|persistence_kind| {
                        (
                            format!(
                                "resident slot parked pid={} slot={} backend={} kind={} hint_bytes={} snapshot={}",
                                job.pid,
                                job.slot_id,
                                job.backend_id,
                                persistence_kind.as_str(),
                                job.pressure_bytes,
                                job.target.final_path.display()
                            ),
                            persistence_kind,
                        )
                    });

                    let event = match result {
                        Ok((msg, persistence_kind)) => SwapResult {
                            pid: job.pid,
                            slot_id: job.slot_id,
                            success: true,
                            detail: msg,
                            persistence_kind,
                            final_path: job.target.final_path.clone(),
                        },
                        Err(err) => SwapResult {
                            pid: job.pid,
                            slot_id: job.slot_id,
                            success: false,
                            detail: format!(
                                "swap failed pid={} slot={}: {}",
                                job.pid, job.slot_id, err
                            ),
                            persistence_kind: SlotPersistenceKind::Unknown,
                            final_path: job.target.final_path.clone(),
                        },
                    };

                    if tx_result.send(event).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| MemoryError::Swap(format!("Failed to spawn swap worker: {}", e)))?;

        self.enabled = true;
        self.tx = Some(tx_job);
        self.rx = Some(rx_result);
        Ok(())
    }

    // ── Query ───────────────────────────────────────────────────────

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_pid_waiting(&self, pid: u64) -> bool {
        self.waiting.contains(&pid)
    }

    pub fn waiting_count(&self) -> usize {
        self.waiting.len()
    }

    pub fn worker_crashes(&self) -> u64 {
        self.worker_crashes
    }

    pub fn remove_waiting(&mut self, pid: u64) {
        self.waiting.remove(&pid);
    }

    // ── Enqueue ─────────────────────────────────────────────────────

    /// Send a swap job to the background worker.
    /// Marks the PID as waiting. Returns a user-facing message.
    pub fn enqueue(
        &mut self,
        pid: u64,
        slot_id: ContextSlotId,
        backend_id: &str,
        family: PromptFamily,
        pressure_bytes: usize,
    ) -> Result<String, MemoryError> {
        let Some(tx) = &self.tx else {
            return Err(MemoryError::Swap("Swap queue is not available".to_string()));
        };

        self.waiting.insert(pid);

        let target =
            restore::prepare_swap_target(&self.dir, pid, slot_id).map_err(MemoryError::Swap)?;
        tx.send(SwapJob {
            pid,
            slot_id,
            backend_id: backend_id.to_string(),
            family,
            target,
            pressure_bytes,
        })
        .map_err(|e| {
            self.waiting.remove(&pid);
            MemoryError::Swap(format!("Failed to enqueue swap job for PID {}: {}", pid, e))
        })?;

        Ok(format!(
            "resident slot PID {} slot {} queued for async parking ({} bytes hinted)",
            pid, slot_id, pressure_bytes
        ))
    }

    // ── Poll completions ────────────────────────────────────────────

    /// Non-blocking drain of completed swap results.
    /// Returns events and accumulated counter deltas for the caller to apply.
    pub fn poll_events(&mut self) -> (Vec<SwapEvent>, SwapCounterDeltas) {
        let mut events = Vec::new();
        let mut deltas = SwapCounterDeltas::zero();

        let Some(rx) = &self.rx else {
            return (events, deltas);
        };

        loop {
            match rx.try_recv() {
                Ok(result) => {
                    self.waiting.remove(&result.pid);
                    if result.success {
                        deltas.swap_count_inc += 1;
                    } else {
                        deltas.swap_failures_inc += 1;
                    }
                    events.push(SwapEvent {
                        pid: result.pid,
                        slot_id: result.slot_id,
                        success: result.success,
                        detail: result.detail,
                        persistence_kind: result.persistence_kind,
                        swap_path: result.success.then_some(result.final_path),
                    });
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.worker_crashes += 1;
                    tracing::error!(
                        "Swap worker thread crashed (crash #{}) — attempting re-spawn",
                        self.worker_crashes,
                    );
                    // Mark all in-flight PIDs as failed
                    let lost_pids: Vec<u64> = self.waiting.drain().collect();
                    for pid in lost_pids {
                        deltas.swap_failures_inc += 1;
                        events.push(SwapEvent {
                            pid,
                            slot_id: 0,
                            success: false,
                            detail: format!("swap job lost: worker crash (pid={})", pid),
                            persistence_kind: SlotPersistenceKind::Unknown,
                            swap_path: None,
                        });
                    }
                    // Attempt one re-spawn
                    self.tx = None;
                    self.rx = None;
                    if self.worker_crashes <= 1 {
                        if let Err(e) = self.spawn_worker() {
                            tracing::error!("Swap worker re-spawn failed: {}", e);
                            self.enabled = false;
                        }
                    } else {
                        tracing::error!(
                            "Swap worker crashed {} times — disabling swap",
                            self.worker_crashes
                        );
                        self.enabled = false;
                    }
                    break;
                }
            }
        }

        (events, deltas)
    }

    // ── Persistence helpers (delegate to restore) ───────────────────

    #[cfg(test)]
    pub fn persist_payload(
        base_dir: &std::path::Path,
        pid: u64,
        slot_id: ContextSlotId,
        backend_id: &str,
    ) -> Result<(PathBuf, SlotPersistenceKind), String> {
        let target = restore::prepare_swap_target(base_dir, pid, slot_id)?;
        let persistence_kind = backend::persist_context_slot_payload_for_backend(
            backend_id,
            PromptFamily::Unknown,
            slot_id,
            &target.final_path,
        )?;
        Ok((target.final_path, persistence_kind))
    }
}
