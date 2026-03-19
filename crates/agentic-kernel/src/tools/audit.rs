use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::kernel_config;
use serde::Serialize;

use super::invocation::ToolContext;
use super::path_guard::workspace_root;
use super::policy::SandboxMode;

#[derive(Serialize)]
struct AuditLogLine<'a> {
    format: &'static str,
    ts_ms: u128,
    pid: u64,
    mode: &'a str,
    caller: &'a str,
    transport: &'a str,
    tool_call_id: Option<&'a str>,
    tool_name: Option<&'a str>,
    success: bool,
    kill: bool,
    duration_ms: u128,
    cmd: &'a str,
    detail: &'a str,
}

pub(crate) struct ToolAuditRecord<'a> {
    pub(crate) pid: u64,
    pub(crate) mode: SandboxMode,
    pub(crate) command: &'a str,
    pub(crate) success: bool,
    pub(crate) duration_ms: u128,
    pub(crate) should_kill: bool,
    pub(crate) detail: &'a str,
    pub(crate) context: &'a ToolContext,
    pub(crate) tool_call_id: Option<&'a str>,
    pub(crate) tool_name: Option<&'a str>,
}

pub(crate) fn append_audit_log(record: ToolAuditRecord<'_>) {
    let root = match workspace_root() {
        Ok(path) => path,
        Err(_) => return,
    };

    let log_path = root.join(&kernel_config().tools.audit_log_file);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis();
    let mode_label = format!("{:?}", record.mode);
    let line = serde_json::to_string(&AuditLogLine {
        format: "jsonl-v1",
        ts_ms: ts,
        pid: record.pid,
        mode: &mode_label,
        caller: record.context.caller.as_str(),
        transport: record.context.transport.as_str(),
        tool_call_id: record.tool_call_id,
        tool_name: record.tool_name,
        success: record.success,
        kill: record.should_kill,
        duration_ms: record.duration_ms,
        cmd: record.command,
        detail: record.detail,
    })
    .unwrap_or_else(|_| {
        format!(
            "{{\"format\":\"jsonl-v1\",\"ts_ms\":{},\"pid\":{},\"mode\":\"{:?}\",\"caller\":{:?},\"transport\":{:?},\"tool_call_id\":{:?},\"tool_name\":{:?},\"success\":{},\"kill\":{},\"duration_ms\":{},\"cmd\":{:?},\"detail\":{:?}}}",
            ts,
            record.pid,
            record.mode,
            record.context.caller.as_str(),
            record.context.transport.as_str(),
            record.tool_call_id,
            record.tool_name,
            record.success,
            record.should_kill,
            record.duration_ms,
            record.command,
            record.detail
        )
    }) + "\n";

    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}
