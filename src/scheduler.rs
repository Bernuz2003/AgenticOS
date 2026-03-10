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

use crate::model_catalog::WorkloadClass;
use crate::process::{ContextPolicy, ContextState};

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
    pub state: String,
    pub token_count: usize,
    pub max_tokens: usize,
    pub context_policy: ContextPolicy,
    pub context_state: ContextState,
}

// ── ProcessScheduler ────────────────────────────────────────────────────

pub struct ProcessScheduler {
    priorities: HashMap<u64, ProcessPriority>,
    quotas: HashMap<u64, ProcessQuota>,
    accounting: HashMap<u64, ResourceAccounting>,
    restored_processes: HashMap<u64, RestoredProcessMetadata>,
}

impl ProcessScheduler {
    pub fn new() -> Self {
        Self {
            priorities: HashMap::new(),
            quotas: HashMap::new(),
            accounting: HashMap::new(),
            restored_processes: HashMap::new(),
        }
    }

    // ── Registration / removal ──────────────────────────────────────────

    /// Register a new process with its workload class.
    /// Sets default priority (`Normal`) and workload-class-aware quotas.
    pub fn register(
        &mut self,
        pid: u64,
        workload: WorkloadClass,
        priority: ProcessPriority,
    ) {
        self.priorities.insert(pid, priority);
        self.quotas.insert(pid, ProcessQuota::defaults_for(workload));
        self.accounting.insert(pid, ResourceAccounting::new(workload));
        self.restored_processes.remove(&pid);
    }

    /// Remove all scheduler state for a process.
    pub fn unregister(&mut self, pid: u64) {
        self.priorities.remove(&pid);
        self.quotas.remove(&pid);
        self.accounting.remove(&pid);
        self.restored_processes.remove(&pid);
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

    // ── Priority ────────────────────────────────────────────────────────

    pub fn priority(&self, pid: u64) -> ProcessPriority {
        self.priorities.get(&pid).copied().unwrap_or(ProcessPriority::Normal)
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
            quota: self.quotas.get(&pid).copied().unwrap_or(ProcessQuota::defaults_for(WorkloadClass::General)),
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
        let by_priority = |p: ProcessPriority| -> usize {
            self.priorities.values().filter(|&&v| v == p).count()
        };
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
        let by_priority = |p: ProcessPriority| -> usize {
            self.priorities.values().filter(|&&v| v == p).count()
        };
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
mod tests {
    use super::*;

    #[test]
    fn register_sets_defaults() {
        let mut sched = ProcessScheduler::new();
        sched.register(1, WorkloadClass::Fast, ProcessPriority::Normal);

        assert_eq!(sched.priority(1), ProcessPriority::Normal);
        let q = sched.quota(1).expect("quota exists");
        assert_eq!(q.max_tokens, 512);
        assert_eq!(q.max_syscalls, 2);
        assert_eq!(sched.tokens_generated(1), 0);
    }

    #[test]
    fn unregister_cleans_up() {
        let mut sched = ProcessScheduler::new();
        sched.register(1, WorkloadClass::General, ProcessPriority::High);
        sched.unregister(1);
        assert!(sched.quota(1).is_none());
        assert!(sched.snapshot(1).is_none());
    }

    #[test]
    fn record_token_enforces_quota() {
        let mut sched = ProcessScheduler::new();
        sched.register(1, WorkloadClass::Fast, ProcessPriority::Normal);
        // Fast workload max_tokens = 512
        for _ in 0..511 {
            assert!(!sched.record_token(1));
        }
        // 512th token -> quota exceeded
        assert!(sched.record_token(1));
    }

    #[test]
    fn record_syscall_enforces_quota() {
        let mut sched = ProcessScheduler::new();
        sched.register(1, WorkloadClass::Fast, ProcessPriority::Normal);
        // Fast workload max_syscalls = 2
        assert!(!sched.record_syscall(1));
        assert!(sched.record_syscall(1));
    }

    #[test]
    fn scheduling_order_respects_priority() {
        let mut sched = ProcessScheduler::new();
        sched.register(1, WorkloadClass::General, ProcessPriority::Low);
        sched.register(2, WorkloadClass::General, ProcessPriority::Critical);
        sched.register(3, WorkloadClass::General, ProcessPriority::Normal);
        sched.register(4, WorkloadClass::General, ProcessPriority::High);

        let order = sched.scheduling_order(&[1, 2, 3, 4]);
        assert_eq!(order, vec![2, 4, 3, 1]);
    }

    #[test]
    fn scheduling_order_tie_breaks_by_pid() {
        let mut sched = ProcessScheduler::new();
        sched.register(5, WorkloadClass::General, ProcessPriority::Normal);
        sched.register(3, WorkloadClass::General, ProcessPriority::Normal);
        sched.register(7, WorkloadClass::General, ProcessPriority::Normal);

        let order = sched.scheduling_order(&[5, 3, 7]);
        assert_eq!(order, vec![3, 5, 7]);
    }

    #[test]
    fn set_priority_changes_order() {
        let mut sched = ProcessScheduler::new();
        sched.register(1, WorkloadClass::General, ProcessPriority::Normal);
        sched.register(2, WorkloadClass::General, ProcessPriority::Normal);

        assert!(sched.set_priority(2, ProcessPriority::High));
        let order = sched.scheduling_order(&[1, 2]);
        assert_eq!(order, vec![2, 1]);
    }

    #[test]
    fn set_quota_override() {
        let mut sched = ProcessScheduler::new();
        sched.register(1, WorkloadClass::Fast, ProcessPriority::Normal);

        let custom = ProcessQuota { max_tokens: 100, max_syscalls: 3 };
        assert!(sched.set_quota(1, custom));

        // max_syscalls = 3: first two calls ok, third exceeds
        assert!(!sched.record_syscall(1));
        assert!(!sched.record_syscall(1));
        assert!(sched.record_syscall(1));
    }

    #[test]
    fn workload_defaults_vary() {
        let fast = ProcessQuota::defaults_for(WorkloadClass::Fast);
        let reasoning = ProcessQuota::defaults_for(WorkloadClass::Reasoning);
        assert!(reasoning.max_tokens > fast.max_tokens);
    }

    #[test]
    fn priority_from_str_loose() {
        assert_eq!(ProcessPriority::from_str_loose("HIGH"), Some(ProcessPriority::High));
        assert_eq!(ProcessPriority::from_str_loose("low"), Some(ProcessPriority::Low));
        assert_eq!(ProcessPriority::from_str_loose("unknown"), None);
    }

    #[test]
    fn snapshot_returns_correct_data() {
        let mut sched = ProcessScheduler::new();
        sched.register(42, WorkloadClass::Code, ProcessPriority::High);
        sched.record_token(42);
        sched.record_token(42);
        sched.record_syscall(42);

        let snap = sched.snapshot(42).expect("snapshot should exist");
        assert_eq!(snap.tokens_generated, 2);
        assert_eq!(snap.syscalls_used, 1);
        assert_eq!(snap.priority, ProcessPriority::High);
        assert!(matches!(snap.workload, WorkloadClass::Code));
    }

    #[test]
    fn summary_counts_priorities() {
        let mut sched = ProcessScheduler::new();
        sched.register(1, WorkloadClass::General, ProcessPriority::Normal);
        sched.register(2, WorkloadClass::General, ProcessPriority::High);
        sched.register(3, WorkloadClass::General, ProcessPriority::High);

        let s = sched.summary();
        assert!(s.contains("scheduler_tracked=3"));
        assert!(s.contains("priority_high=2"));
        assert!(s.contains("priority_normal=1"));
    }

    #[test]
    fn registered_pids_returns_sorted() {
        let mut sched = ProcessScheduler::new();
        sched.register(5, WorkloadClass::General, ProcessPriority::Normal);
        sched.register(1, WorkloadClass::Code, ProcessPriority::High);
        sched.register(3, WorkloadClass::Fast, ProcessPriority::Low);

        let pids = sched.registered_pids();
        assert_eq!(pids, vec![1, 3, 5]);

        sched.unregister(3);
        let pids = sched.registered_pids();
        assert_eq!(pids, vec![1, 5]);
    }
}
