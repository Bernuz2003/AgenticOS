use super::planner::DeadlineReason;

impl DeadlineReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RemoteTimeout => "remote_timeout",
            Self::SyscallTimeout => "syscall_timeout",
            Self::Checkpoint => "checkpoint",
            Self::ScheduledJob => "scheduled_job",
            Self::ScheduledJobTimeout => "scheduled_job_timeout",
        }
    }
}
