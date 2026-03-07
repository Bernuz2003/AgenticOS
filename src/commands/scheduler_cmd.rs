use crate::protocol;
use crate::scheduler::ProcessPriority;

use super::context::CommandContext;
use super::metrics::log_event;

use serde_json;

pub(crate) fn handle_set_priority(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    let parts: Vec<&str> = payload_text.splitn(2, char::is_whitespace).collect();
    if parts.len() != 2 {
        protocol::response_err_code(
            "SET_PRIORITY_INVALID",
            "SET_PRIORITY requires: <PID> <low|normal|high|critical>",
        )
    } else if let Ok(pid) = parts[0].parse::<u64>() {
        if let Some(level) = ProcessPriority::from_str_loose(parts[1].trim()) {
            if ctx.scheduler.set_priority(pid, level) {
                log_event("set_priority", ctx.client_id, Some(pid), &format!("priority={}", level));
                protocol::response_ok_code(
                    "SET_PRIORITY",
                    &format!("PID {} priority set to {}", pid, level),
                )
            } else {
                protocol::response_err_code(
                    "PID_NOT_FOUND",
                    &format!("PID {} not tracked by scheduler", pid),
                )
            }
        } else {
            protocol::response_err_code(
                "SET_PRIORITY_INVALID",
                &format!("Unknown priority level '{}'. Use: low, normal, high, critical", parts[1]),
            )
        }
    } else {
        protocol::response_err_code("SET_PRIORITY_INVALID", "PID must be numeric")
    }
}

pub(crate) fn handle_get_quota(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if let Ok(pid) = payload_text.parse::<u64>() {
        if let Some(snap) = ctx.scheduler.snapshot(pid) {
            let json = serde_json::json!({
                "pid": pid,
                "priority": format!("{}", snap.priority),
                "workload": format!("{:?}", snap.workload),
                "max_tokens": snap.quota.max_tokens,
                "max_syscalls": snap.quota.max_syscalls,
                "tokens_generated": snap.tokens_generated,
                "syscalls_used": snap.syscalls_used,
                "elapsed_secs": format!("{:.2}", snap.elapsed_secs).parse::<f64>().unwrap_or(snap.elapsed_secs),
            });
            protocol::response_ok_code("GET_QUOTA", &json.to_string())
        } else {
            protocol::response_err_code(
                "PID_NOT_FOUND",
                &format!("PID {} not tracked by scheduler", pid),
            )
        }
    } else {
        protocol::response_err_code("GET_QUOTA_INVALID", "GET_QUOTA requires numeric PID")
    }
}

pub(crate) fn handle_set_quota(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    let parts: Vec<&str> = payload_text.splitn(2, char::is_whitespace).collect();
    if parts.len() != 2 {
        protocol::response_err_code(
            "SET_QUOTA_INVALID",
            "SET_QUOTA requires: <PID> <max_tokens=N,max_syscalls=N>",
        )
    } else if let Ok(pid) = parts[0].parse::<u64>() {
        if let Some(current) = ctx.scheduler.quota(pid).copied() {
            let mut new_quota = current;
            let mut parse_ok = true;
            for kv in parts[1].split(',') {
                let kv = kv.trim();
                if let Some((k, v)) = kv.split_once('=') {
                    match k.trim() {
                        "max_tokens" => {
                            if let Ok(val) = v.trim().parse::<usize>() {
                                new_quota.max_tokens = val;
                            } else {
                                parse_ok = false;
                            }
                        }
                        "max_syscalls" => {
                            if let Ok(val) = v.trim().parse::<usize>() {
                                new_quota.max_syscalls = val;
                            } else {
                                parse_ok = false;
                            }
                        }
                        _ => { parse_ok = false; }
                    }
                } else {
                    parse_ok = false;
                }
            }
            if parse_ok {
                ctx.scheduler.set_quota(pid, new_quota);
                log_event("set_quota", ctx.client_id, Some(pid),
                    &format!("max_tokens={} max_syscalls={}", new_quota.max_tokens, new_quota.max_syscalls));
                protocol::response_ok_code(
                    "SET_QUOTA",
                    &format!("PID {} quota updated: max_tokens={} max_syscalls={}",
                        pid, new_quota.max_tokens, new_quota.max_syscalls),
                )
            } else {
                protocol::response_err_code(
                    "SET_QUOTA_INVALID",
                    "Invalid quota format. Use: max_tokens=N,max_syscalls=N",
                )
            }
        } else {
            protocol::response_err_code(
                "PID_NOT_FOUND",
                &format!("PID {} not tracked by scheduler", pid),
            )
        }
    } else {
        protocol::response_err_code("SET_QUOTA_INVALID", "PID must be numeric")
    }
}
