use mio::Token;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::runtime::deadlines::{DeadlineReason, NextDeadline};
use crate::runtimes::RuntimeRegistry;

pub(crate) const SERVER: Token = Token(0);
pub(crate) const WORKER_WAKE_TOKEN: Token = Token(1);

#[derive(Debug, Clone, Copy)]
pub(crate) enum LoopWakeReason {
    Network,
    Worker,
    Deadline(DeadlineReason),
    SpuriousWake,
}

impl LoopWakeReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Worker => "worker",
            Self::Deadline(reason) => reason.as_str(),
            Self::SpuriousWake => "spurious_wake",
        }
    }
}

pub(crate) fn classify_wake_reason(
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

pub(crate) fn instant_for_timestamp(now: Instant, now_ms: i64, target_ms: i64) -> Instant {
    if target_ms <= now_ms {
        return now;
    }

    now + Duration::from_millis(target_ms.saturating_sub(now_ms) as u64)
}

pub(crate) fn refresh_syscall_wait_tracking(
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
