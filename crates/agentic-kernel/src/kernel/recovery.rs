use std::io;

use crate::audit::{self, AuditContext};
use crate::storage::{BootRecoveryReport, StorageService};

pub(crate) fn run_boot_recovery(storage: &mut StorageService) -> io::Result<BootRecoveryReport> {
    let report = storage.run_boot_recovery().map_err(io::Error::other)?;
    audit::record(
        storage,
        audit::KERNEL_BOOT_RECOVERED,
        format!(
            "sessions={} runtimes={} reset_sessions={} interrupted_runs={} interrupted_turns={} logical_resume={} strong_restore_candidates={} pending_queue={}",
            report.persisted_sessions,
            report.known_runtimes,
            report.stale_active_sessions_reset,
            report.interrupted_process_runs,
            report.interrupted_turns,
            report.logical_resume_sessions,
            report.strong_restore_candidate_sessions,
            report.pending_runtime_queue_entries
        ),
        AuditContext::default(),
    );
    Ok(report)
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;

