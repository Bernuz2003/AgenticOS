use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::kernel_config;
use serde::Serialize;

use super::path_guard::workspace_root;
use super::policy::SandboxMode;

#[derive(Serialize)]
struct AuditLogLine<'a> {
    format: &'static str,
    ts_ms: u128,
    pid: u64,
    mode: &'a str,
    success: bool,
    kill: bool,
    duration_ms: u128,
    cmd: &'a str,
    detail: &'a str,
}

pub(crate) fn append_audit_log(
    pid: u64,
    mode: SandboxMode,
    command: &str,
    success: bool,
    duration_ms: u128,
    should_kill: bool,
    detail: &str,
) {
    let root = match workspace_root() {
        Ok(path) => path,
        Err(_) => return,
    };

    let log_path = root.join(&kernel_config().tools.audit_log_file);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis();
    let mode_label = format!("{mode:?}");
    let line = serde_json::to_string(&AuditLogLine {
        format: "jsonl-v1",
        ts_ms: ts,
        pid,
        mode: &mode_label,
        success,
        kill: should_kill,
        duration_ms,
        cmd: command,
        detail,
    })
    .unwrap_or_else(|_| {
        format!(
            "{{\"format\":\"jsonl-v1\",\"ts_ms\":{},\"pid\":{},\"mode\":\"{:?}\",\"success\":{},\"kill\":{},\"duration_ms\":{},\"cmd\":{:?},\"detail\":{:?}}}",
            ts, pid, mode, success, should_kill, duration_ms, command, detail
        )
    }) + "\n";

    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}
