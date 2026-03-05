//! Async swap manager for NeuralMemory.
//!
//! Owns the swap worker thread, job queue, and waiting-PID tracking.
//! Extracted from `core.rs` (M13) to keep the allocator focused on
//! block management and process bookkeeping.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use super::swap_io;
use super::types::SwapEvent;
use crate::errors::MemoryError;

// ── Internal message types ──────────────────────────────────────────────

pub(super) struct SwapJob {
    pub pid: u64,
    pub payload: Vec<u8>,
}

struct SwapResult {
    pid: u64,
    success: bool,
    detail: String,
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
}

impl SwapManager {
    pub fn new() -> Self {
        Self {
            enabled: false,
            dir: PathBuf::from("workspace/swap"),
            tx: None,
            rx: None,
            waiting: HashSet::new(),
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

        let validated = swap_io::resolve_valid_swap_dir(swap_dir)
            .map_err(MemoryError::Swap)?;
        self.dir = validated.clone();

        let (tx_job, rx_job) = mpsc::channel::<SwapJob>();
        let (tx_result, rx_result) = mpsc::channel::<SwapResult>();
        let worker_dir = validated;

        thread::Builder::new()
            .name("agentic_swap_worker".to_string())
            .spawn(move || {
                while let Ok(job) = rx_job.recv() {
                    let now_ns = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0);
                    let file_stem = format!("pid_{}_{}", job.pid, now_ns);

                    let result =
                        swap_io::persist_swap_payload(&worker_dir, &file_stem, &job.payload)
                            .map(|final_path| {
                                format!(
                                    "swap persisted pid={} bytes={} file={}",
                                    job.pid,
                                    job.payload.len(),
                                    final_path.display()
                                )
                            });

                    let event = match result {
                        Ok(msg) => SwapResult {
                            pid: job.pid,
                            success: true,
                            detail: msg,
                        },
                        Err(err) => SwapResult {
                            pid: job.pid,
                            success: false,
                            detail: format!("swap failed pid={}: {}", job.pid, err),
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

    pub fn remove_waiting(&mut self, pid: u64) {
        self.waiting.remove(&pid);
    }

    // ── Enqueue ─────────────────────────────────────────────────────

    /// Send a swap job to the background worker.
    /// Marks the PID as waiting. Returns a user-facing message.
    pub fn enqueue(&mut self, pid: u64, payload: Vec<u8>) -> Result<String, MemoryError> {
        let Some(tx) = &self.tx else {
            return Err(MemoryError::Swap("Swap queue is not available".to_string()));
        };

        self.waiting.insert(pid);

        let payload_len = payload.len();
        tx.send(SwapJob { pid, payload })
            .map_err(|e| {
                self.waiting.remove(&pid);
                MemoryError::Swap(format!(
                    "Failed to enqueue swap job for PID {}: {}",
                    pid, e
                ))
            })?;

        Ok(format!(
            "OOM: PID {} queued for async swap ({} bytes)",
            pid, payload_len
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
                        success: result.success,
                        detail: result.detail,
                    });
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.enabled = false;
                    self.tx = None;
                    self.rx = None;
                    break;
                }
            }
        }

        (events, deltas)
    }

    // ── Persistence helpers (delegate to swap_io) ───────────────────

    #[allow(dead_code)]
    pub fn persist_payload(
        base_dir: &std::path::Path,
        file_stem: &str,
        payload: &[u8],
    ) -> Result<PathBuf, String> {
        swap_io::persist_swap_payload(base_dir, file_stem, payload)
    }
}
