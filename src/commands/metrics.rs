use std::sync::Mutex;
use std::time::Instant;

#[derive(Default)]
struct MetricsState {
    total_commands: u64,
    total_errors: u64,
    total_exec_started: u64,
    total_signals: u64,
}

fn metrics_state() -> &'static Mutex<MetricsState> {
    static METRICS: std::sync::OnceLock<Mutex<MetricsState>> = std::sync::OnceLock::new();
    METRICS.get_or_init(|| Mutex::new(MetricsState::default()))
}

fn metrics_start() -> &'static Instant {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now)
}

pub(crate) fn record_command(success: bool) {
    let mut lock = metrics_state().lock().unwrap();
    lock.total_commands += 1;
    if !success {
        lock.total_errors += 1;
    }
}

pub(crate) fn inc_exec_started() {
    let mut lock = metrics_state().lock().unwrap();
    lock.total_exec_started += 1;
}

pub(crate) fn inc_signal_count() {
    let mut lock = metrics_state().lock().unwrap();
    lock.total_signals += 1;
}

pub(crate) fn snapshot_metrics() -> (u64, u64, u64, u64, u64) {
    let lock = metrics_state().lock().unwrap();
    (
        metrics_start().elapsed().as_secs(),
        lock.total_commands,
        lock.total_errors,
        lock.total_exec_started,
        lock.total_signals,
    )
}

pub(crate) fn log_event(event: &str, client_id: usize, pid: Option<u64>, detail: &str) {
    let pid_text = pid
        .map(|p| p.to_string())
        .unwrap_or_else(|| "-".to_string());
    eprintln!(
        "event={} client_id={} pid={} detail=\"{}\"",
        event,
        client_id,
        pid_text,
        detail.replace('"', "'")
    );
}
