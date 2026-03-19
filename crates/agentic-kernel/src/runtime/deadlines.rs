use std::cmp::Ordering;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlineReason {
    RemoteTimeout,
    SyscallTimeout,
    Checkpoint,
}

impl DeadlineReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RemoteTimeout => "remote_timeout",
            Self::SyscallTimeout => "syscall_timeout",
            Self::Checkpoint => "checkpoint",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeadlineCandidate {
    pub reason: DeadlineReason,
    pub at: Instant,
    pub subject_pid: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub struct NextDeadline {
    pub reason: DeadlineReason,
    pub at: Instant,
    pub subject_pid: Option<u64>,
}

pub fn pick_next_deadline(candidates: &[DeadlineCandidate]) -> Option<NextDeadline> {
    candidates
        .iter()
        .copied()
        .min_by(|left, right| match left.at.cmp(&right.at) {
            Ordering::Equal => left.reason.as_str().cmp(right.reason.as_str()),
            other => other,
        })
        .map(|candidate| NextDeadline {
            reason: candidate.reason,
            at: candidate.at,
            subject_pid: candidate.subject_pid,
        })
}

pub fn compute_poll_timeout(now: Instant, next_deadline: Option<NextDeadline>) -> Option<Duration> {
    match next_deadline {
        None => None,
        Some(next) if next.at <= now => Some(Duration::from_millis(0)),
        Some(next) => {
            let until_deadline = next.at.saturating_duration_since(now);
            Some(until_deadline)
        }
    }
}
