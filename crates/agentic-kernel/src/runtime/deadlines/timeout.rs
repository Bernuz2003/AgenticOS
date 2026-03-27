use std::time::{Duration, Instant};

use super::planner::NextDeadline;

pub fn compute_poll_timeout(now: Instant, next_deadline: Option<NextDeadline>) -> Option<Duration> {
    match next_deadline {
        None => None,
        Some(next) if next.at <= now => Some(Duration::from_millis(0)),
        Some(next) => Some(next.at.saturating_duration_since(now)),
    }
}
