use std::cmp::Ordering;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlineReason {
    RemoteTimeout,
    SyscallTimeout,
    Checkpoint,
    ScheduledJob,
    ScheduledJobTimeout,
}

#[derive(Debug, Clone, Copy)]
pub struct DeadlineCandidate {
    pub reason: DeadlineReason,
    pub at: Instant,
    pub subject_id: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub struct NextDeadline {
    pub reason: DeadlineReason,
    pub at: Instant,
    pub subject_id: Option<u64>,
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
            subject_id: candidate.subject_id,
        })
}
