/// Process Scheduler — priority, quotas, resource accounting.
///
/// Centralises per-PID resource governance that was previously scattered
/// across `tools.rs` (rate limit) and `memory/core.rs` (token-slot quota).
///
/// Design notes:
/// - The scheduler does **not** own processes; it maintains metadata (priority,
///   quotas, accounting counters) keyed by PID.
/// - `run_engine_tick` queries `scheduling_order()` to step processes in
///   descending priority order.
/// - Quota enforcement is checked *before* each step / syscall.
use std::collections::HashMap;
use std::time::Instant;

use crate::backend::BackendCapabilities;
use crate::model_catalog::WorkloadClass;
use crate::process::{ContextPolicy, ContextState, ContextStatusSnapshot, HumanInputRequest};
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};

// ── Priority ────────────────────────────────────────────────────────────

/// Scheduling priority for a process.  Higher value ⇒ stepped first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProcessPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl ProcessPriority {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "low" => Some(Self::Low),
            "normal" => Some(Self::Normal),
            "high" => Some(Self::High),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

impl std::fmt::Display for ProcessPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Critical => "critical",
        };
        f.write_str(label)
    }
}

// ── Quota ───────────────────────────────────────────────────────────────

/// Hard resource limits for one process.
#[derive(Debug, Clone, Copy)]
pub struct ProcessQuota {
    /// Maximum generation tokens before the process is force-terminated.
    pub max_tokens: usize,
    /// Maximum number of syscalls (tool invocations) allowed.
    pub max_syscalls: usize,
}

impl ProcessQuota {
    /// Returns sensible defaults per workload class.
    pub fn defaults_for(workload: WorkloadClass) -> Self {
        let (max_tokens, max_syscalls) = crate::policy::scheduler_quota_defaults(workload);

        Self {
            max_tokens,
            max_syscalls,
        }
    }
}

// ── Accounting ──────────────────────────────────────────────────────────

/// Runtime counters for a single process.
#[derive(Debug, Clone)]
pub struct ResourceAccounting {
    pub tokens_generated: usize,
    pub syscalls_used: usize,
    pub started_at: Instant,
    pub workload: WorkloadClass,
}

impl ResourceAccounting {
    pub fn new(workload: WorkloadClass) -> Self {
        Self {
            tokens_generated: 0,
            syscalls_used: 0,
            started_at: Instant::now(),
            workload,
        }
    }

    /// Elapsed wall-clock seconds since process start.
    pub fn elapsed_secs(&self) -> f64 {
        self.started_at.elapsed().as_secs_f64()
    }
}

// ── Scheduler snapshot (for STATUS) ─────────────────────────────────────

/// Read-only snapshot of scheduler state for a single process.
#[derive(Debug, Clone)]
pub struct ProcessSchedulerSnapshot {
    pub priority: ProcessPriority,
    pub quota: ProcessQuota,
    pub tokens_generated: usize,
    pub syscalls_used: usize,
    pub elapsed_secs: f64,
    pub workload: WorkloadClass,
}

#[derive(Debug, Clone)]
pub struct RestoredProcessMetadata {
    pub owner_id: usize,
    pub tool_caller: ToolCaller,
    pub permission_policy: ProcessPermissionPolicy,
    pub state: String,
    pub token_count: usize,
    pub max_tokens: usize,
    pub context_slot_id: Option<u64>,
    pub resident_slot_policy: Option<String>,
    pub resident_slot_state: Option<String>,
    pub resident_slot_snapshot_path: Option<String>,
    pub backend_id: Option<String>,
    pub backend_class: Option<String>,
    pub backend_capabilities: Option<BackendCapabilities>,
    pub context_policy: ContextPolicy,
    pub context_state: ContextState,
    pub pending_human_request: Option<HumanInputRequest>,
}

#[derive(Debug, Clone)]
pub struct CheckedOutProcessMetadata {
    pub owner_id: usize,
    pub tool_caller: ToolCaller,
    pub permission_policy: ProcessPermissionPolicy,
    pub state: String,
    pub checked_out_at: Instant,
    pub tokens: usize,
    pub index_pos: usize,
    pub max_tokens: usize,
    pub context_slot_id: Option<u64>,
    pub resident_slot_policy: Option<String>,
    pub resident_slot_state: Option<String>,
    pub resident_slot_snapshot_path: Option<String>,
    pub backend_id: Option<String>,
    pub backend_class: Option<String>,
    pub backend_capabilities: Option<BackendCapabilities>,
    pub context: ContextStatusSnapshot,
    pub pending_human_request: Option<HumanInputRequest>,
}

// ── ProcessScheduler ────────────────────────────────────────────────────

pub struct ProcessScheduler {
    priorities: HashMap<u64, ProcessPriority>,
    quotas: HashMap<u64, ProcessQuota>,
    accounting: HashMap<u64, ResourceAccounting>,
    restored_processes: HashMap<u64, RestoredProcessMetadata>,
    checked_out_processes: HashMap<u64, CheckedOutProcessMetadata>,
}

impl ProcessScheduler {
    pub fn new() -> Self {
        Self {
            priorities: HashMap::new(),
            quotas: HashMap::new(),
            accounting: HashMap::new(),
            restored_processes: HashMap::new(),
            checked_out_processes: HashMap::new(),
        }
    }

    // ── Registration / removal ──────────────────────────────────────────

    /// Register a new process with its workload class.
    /// Sets default priority (`Normal`) and workload-class-aware quotas.
    pub fn register(&mut self, pid: u64, workload: WorkloadClass, priority: ProcessPriority) {
        self.priorities.insert(pid, priority);
        self.quotas
            .insert(pid, ProcessQuota::defaults_for(workload));
        self.accounting
            .insert(pid, ResourceAccounting::new(workload));
        self.restored_processes.remove(&pid);
        self.checked_out_processes.remove(&pid);
    }

    /// Remove all scheduler state for a process.
    pub fn unregister(&mut self, pid: u64) {
        self.priorities.remove(&pid);
        self.quotas.remove(&pid);
        self.accounting.remove(&pid);
        self.restored_processes.remove(&pid);
        self.checked_out_processes.remove(&pid);
    }

    pub fn clear_restored_processes(&mut self) {
        self.restored_processes.clear();
    }

    pub fn record_restored_process(&mut self, pid: u64, metadata: RestoredProcessMetadata) {
        self.restored_processes.insert(pid, metadata);
    }

    pub fn restored_process(&self, pid: u64) -> Option<&RestoredProcessMetadata> {
        self.restored_processes.get(&pid)
    }

    pub fn restored_pids(&self) -> Vec<u64> {
        let mut pids: Vec<u64> = self.restored_processes.keys().copied().collect();
        pids.sort_unstable();
        pids
    }

    pub fn record_checked_out_process(&mut self, pid: u64, metadata: CheckedOutProcessMetadata) {
        self.checked_out_processes.insert(pid, metadata);
    }

    pub fn clear_checked_out_process(&mut self, pid: u64) {
        self.checked_out_processes.remove(&pid);
    }

    pub fn checked_out_process(&self, pid: u64) -> Option<&CheckedOutProcessMetadata> {
        self.checked_out_processes.get(&pid)
    }

    pub fn checked_out_snapshots(&self) -> Vec<(u64, CheckedOutProcessMetadata)> {
        self.checked_out_processes
            .iter()
            .map(|(pid, metadata)| (*pid, metadata.clone()))
            .collect()
    }

    // ── Priority ────────────────────────────────────────────────────────

    pub fn priority(&self, pid: u64) -> ProcessPriority {
        self.priorities
            .get(&pid)
            .copied()
            .unwrap_or(ProcessPriority::Normal)
    }

    pub fn set_priority(&mut self, pid: u64, priority: ProcessPriority) -> bool {
        if let std::collections::hash_map::Entry::Occupied(mut e) = self.priorities.entry(pid) {
            e.insert(priority);
            true
        } else {
            false
        }
    }

    // ── Quota read / write ──────────────────────────────────────────────

    pub fn quota(&self, pid: u64) -> Option<&ProcessQuota> {
        self.quotas.get(&pid)
    }

    pub fn set_quota(&mut self, pid: u64, quota: ProcessQuota) -> bool {
        if let std::collections::hash_map::Entry::Occupied(mut e) = self.quotas.entry(pid) {
            e.insert(quota);
            true
        } else {
            false
        }
    }

    // ── Accounting ──────────────────────────────────────────────────────

    /// Record one generated token.  Returns `true` if quota is exceeded.
    pub fn record_token(&mut self, pid: u64) -> bool {
        if let Some(acc) = self.accounting.get_mut(&pid) {
            acc.tokens_generated += 1;
            if let Some(q) = self.quotas.get(&pid) {
                return acc.tokens_generated >= q.max_tokens;
            }
        }
        false
    }

    /// Record one syscall invocation.  Returns `true` if quota is exceeded.
    pub fn record_syscall(&mut self, pid: u64) -> bool {
        if let Some(acc) = self.accounting.get_mut(&pid) {
            acc.syscalls_used += 1;
            if let Some(q) = self.quotas.get(&pid) {
                return acc.syscalls_used >= q.max_syscalls;
            }
        }
        false
    }

    /// Used in tests to verify token accounting.
    #[cfg(test)]
    pub fn tokens_generated(&self, pid: u64) -> usize {
        self.accounting.get(&pid).map_or(0, |a| a.tokens_generated)
    }

    // ── Scheduling order ────────────────────────────────────────────────

    /// Given a set of active PIDs, return them sorted by descending priority.
    /// Ties are broken by PID (lower PID goes first — FIFO-ish).
    pub fn scheduling_order(&self, active_pids: &[u64]) -> Vec<u64> {
        let mut ordered: Vec<(u64, ProcessPriority)> = active_pids
            .iter()
            .map(|&pid| (pid, self.priority(pid)))
            .collect();
        // Sort descending by priority, then ascending by PID for stability.
        ordered.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ordered.into_iter().map(|(pid, _)| pid).collect()
    }

    // ── Snapshot ────────────────────────────────────────────────────────

    /// Return a sorted list of all PIDs tracked by the scheduler.
    pub fn registered_pids(&self) -> Vec<u64> {
        let mut pids: Vec<u64> = self.accounting.keys().copied().collect();
        pids.sort_unstable();
        pids
    }

    pub fn snapshot(&self, pid: u64) -> Option<ProcessSchedulerSnapshot> {
        let acc = self.accounting.get(&pid)?;
        Some(ProcessSchedulerSnapshot {
            priority: self.priority(pid),
            quota: self
                .quotas
                .get(&pid)
                .copied()
                .unwrap_or(ProcessQuota::defaults_for(WorkloadClass::General)),
            tokens_generated: acc.tokens_generated,
            syscalls_used: acc.syscalls_used,
            elapsed_secs: acc.elapsed_secs(),
            workload: acc.workload,
        })
    }

    /// Summary line for the global STATUS response.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn summary(&self) -> String {
        let total = self.accounting.len();
        let by_priority =
            |p: ProcessPriority| -> usize { self.priorities.values().filter(|&&v| v == p).count() };
        format!(
            "scheduler_tracked={} priority_critical={} priority_high={} priority_normal={} priority_low={}",
            total,
            by_priority(ProcessPriority::Critical),
            by_priority(ProcessPriority::High),
            by_priority(ProcessPriority::Normal),
            by_priority(ProcessPriority::Low),
        )
    }

    /// Structured summary for JSON STATUS response.
    pub fn summary_counts(&self) -> (usize, usize, usize, usize, usize) {
        let by_priority =
            |p: ProcessPriority| -> usize { self.priorities.values().filter(|&&v| v == p).count() };
        (
            self.accounting.len(),
            by_priority(ProcessPriority::Critical),
            by_priority(ProcessPriority::High),
            by_priority(ProcessPriority::Normal),
            by_priority(ProcessPriority::Low),
        )
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "scheduler_tests.rs"]
mod tests;
