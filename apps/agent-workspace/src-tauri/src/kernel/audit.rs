use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub fn audit_log_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join("workspace").join("syscall_audit.log")
}

#[derive(Debug, Clone)]
pub struct AuditLogEntry {
    pub pid: u64,
    pub success: bool,
    pub should_kill: bool,
    pub duration_ms: u128,
    pub command: String,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
struct AuditLogJsonLine {
    pid: u64,
    success: bool,
    kill: bool,
    duration_ms: u128,
    cmd: String,
    detail: String,
}

pub fn read_recent_audit_entries_for_pid(
    workspace_root: &Path,
    pid: u64,
    max_entries: usize,
) -> Vec<AuditLogEntry> {
    let Ok(content) = fs::read_to_string(audit_log_path(workspace_root)) else {
        return Vec::new();
    };

    let mut entries: Vec<AuditLogEntry> = content
        .lines()
        .filter_map(parse_audit_log_line)
        .filter(|entry| entry.pid == pid)
        .collect();

    if entries.len() > max_entries {
        entries = entries.split_off(entries.len() - max_entries);
    }

    entries
}

fn parse_audit_log_line(line: &str) -> Option<AuditLogEntry> {
    if let Ok(json_line) = serde_json::from_str::<AuditLogJsonLine>(line) {
        return Some(AuditLogEntry {
            pid: json_line.pid,
            success: json_line.success,
            should_kill: json_line.kill,
            duration_ms: json_line.duration_ms,
            command: json_line.cmd,
            detail: json_line.detail,
        });
    }

    Some(AuditLogEntry {
        pid: extract_unquoted(line, "pid=")?.parse().ok()?,
        success: extract_unquoted(line, "success=")?.parse().ok()?,
        should_kill: extract_unquoted(line, "kill=")?.parse().ok()?,
        duration_ms: extract_unquoted(line, "duration_ms=")?.parse().ok()?,
        command: unescape_log_string(extract_quoted(line, "cmd=")?),
        detail: unescape_log_string(extract_quoted(line, "detail=")?),
    })
}

fn extract_unquoted<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let start = line.find(prefix)? + prefix.len();
    let rest = &line[start..];
    let end = rest.find(' ').unwrap_or(rest.len());
    Some(&rest[..end])
}

fn extract_quoted<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let start = line.find(prefix)? + prefix.len();
    let rest = line.get(start..)?;
    let quoted = rest.strip_prefix('"')?;
    let end = quoted.find('"')?;
    Some(&quoted[..end])
}

fn unescape_log_string(value: &str) -> String {
    value.replace("\\n", "\n").replace("\\\"", "\"")
}

#[cfg(test)]
mod tests {
    use super::parse_audit_log_line;

    #[test]
    fn parse_audit_log_line_extracts_quoted_fields() {
        let line = r#"ts_ms=1 pid=7 mode=Host success=true kill=false duration_ms=12 cmd="[[PYTHON: print(1)]]" detail="Output:\n1""#;
        let parsed = parse_audit_log_line(line).expect("parse audit line");
        assert_eq!(parsed.pid, 7);
        assert!(parsed.success);
        assert_eq!(parsed.command, "[[PYTHON: print(1)]]");
        assert_eq!(parsed.detail, "Output:\n1");
    }

    #[test]
    fn parse_audit_log_line_supports_jsonl_with_quotes() {
        let line = r#"{"format":"jsonl-v1","ts_ms":1,"pid":9,"mode":"Host","success":true,"kill":false,"duration_ms":7,"cmd":"TOOL:python {\"code\":\"print(\\\"hi\\\")\"}","detail":"{\"ok\":true}"}"#;
        let parsed = parse_audit_log_line(line).expect("parse jsonl audit line");
        assert_eq!(parsed.pid, 9);
        assert!(parsed.success);
        assert_eq!(parsed.command, r#"TOOL:python {"code":"print(\"hi\")"}"#);
        assert_eq!(parsed.detail, r#"{"ok":true}"#);
    }
}
