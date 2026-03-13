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
mod tests {
    use super::run_boot_recovery;
    use crate::storage::StorageService;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn boot_recovery_records_global_audit_event() {
        let dir = make_temp_dir("agenticos-kernel-recovery");
        let db_path = dir.join("agenticos.db");

        let mut storage = StorageService::open(&db_path).expect("open storage");
        storage
            .insert_session(
                "sess-1",
                "Recovered session",
                "idle",
                None,
                None,
                1_000,
                1_000,
            )
            .expect("insert session");

        let report = run_boot_recovery(&mut storage).expect("run boot recovery");
        assert_eq!(report.persisted_sessions, 1);

        let audit_events = storage
            .recent_audit_events(8)
            .expect("load recent audit events");
        assert_eq!(audit_events.len(), 1);
        assert_eq!(audit_events[0].category, "kernel");
        assert_eq!(audit_events[0].kind, "boot_recovered");
        assert!(audit_events[0].detail.contains("sessions=1"));

        let _ = fs::remove_dir_all(dir);
    }

    fn make_temp_dir(prefix: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{timestamp}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
