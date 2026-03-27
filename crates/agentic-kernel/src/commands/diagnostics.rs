use std::time::Instant;

/// Kernel-level metrics counters — owned by Kernel, no global statics.
pub(crate) struct MetricsState {
    started_at: Instant,
    pub total_commands: u64,
    pub total_errors: u64,
    pub total_exec_started: u64,
    pub total_signals: u64,
}

impl MetricsState {
    pub fn new() -> Self {
        MetricsState {
            started_at: Instant::now(),
            total_commands: 0,
            total_errors: 0,
            total_exec_started: 0,
            total_signals: 0,
        }
    }

    pub fn record_command(&mut self, success: bool) {
        self.total_commands += 1;
        if !success {
            self.total_errors += 1;
        }
    }

    pub fn inc_exec_started(&mut self) {
        self.total_exec_started += 1;
    }

    pub fn inc_signal_count(&mut self) {
        self.total_signals += 1;
    }

    pub fn snapshot(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.started_at.elapsed().as_secs(),
            self.total_commands,
            self.total_errors,
            self.total_exec_started,
            self.total_signals,
        )
    }
}

pub(crate) fn log_event(event: &str, client_id: usize, pid: Option<u64>, detail: &str) {
    let pid_text = pid
        .map(|p| p.to_string())
        .unwrap_or_else(|| "-".to_string());
    tracing::info!(
        event,
        client_id,
        pid = %pid_text,
        detail = detail.replace('"', "'"),
        "kernel_event"
    );
}
