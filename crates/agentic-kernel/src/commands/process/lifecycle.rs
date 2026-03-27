use crate::protocol;
use crate::scheduler::ProcessPriority;
use agentic_control_models::KernelEvent;
use agentic_protocol::ControlErrorCode;

use crate::commands::context::SchedulerCommandContext;
use crate::commands::diagnostics::log_event;

use serde_json::json;

pub(crate) fn handle_set_priority(ctx: SchedulerCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let SchedulerCommandContext {
        client,
        request_id,
        scheduler,
        pending_events,
        client_id,
    } = ctx;
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    let parts: Vec<&str> = payload_text.splitn(2, char::is_whitespace).collect();
    if parts.len() != 2 {
        protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::SetPriorityInvalid,
            protocol::schema::ERROR,
            "SET_PRIORITY requires: <PID> <low|normal|high|critical>",
        )
    } else if let Ok(pid) = parts[0].parse::<u64>() {
        if let Some(level) = ProcessPriority::from_str_loose(parts[1].trim()) {
            if scheduler.set_priority(pid, level) {
                pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "priority_updated".to_string(),
                });
                log_event(
                    "set_priority",
                    client_id,
                    Some(pid),
                    &format!("priority={}", level),
                );
                let message = format!("PID {} priority set to {}", pid, level);
                protocol::response_protocol_ok(
                    client,
                    request_id,
                    "SET_PRIORITY",
                    protocol::schema::SET_PRIORITY,
                    &json!({"pid": pid, "priority": format!("{}", level)}),
                    Some(&message),
                )
            } else {
                protocol::response_protocol_err_typed(
                    client,
                    request_id,
                    ControlErrorCode::PidNotFound,
                    protocol::schema::ERROR,
                    &format!("PID {} not tracked by scheduler", pid),
                )
            }
        } else {
            protocol::response_protocol_err_typed(
                client,
                request_id,
                ControlErrorCode::SetPriorityInvalid,
                protocol::schema::ERROR,
                &format!(
                    "Unknown priority level '{}'. Use: low, normal, high, critical",
                    parts[1]
                ),
            )
        }
    } else {
        protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::SetPriorityInvalid,
            protocol::schema::ERROR,
            "PID must be numeric",
        )
    }
}

pub(crate) fn handle_get_quota(ctx: SchedulerCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let SchedulerCommandContext {
        client,
        request_id,
        scheduler,
        ..
    } = ctx;
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if let Ok(pid) = payload_text.parse::<u64>() {
        if let Some(snap) = scheduler.snapshot(pid) {
            let json = json!({
                "pid": pid,
                "priority": format!("{}", snap.priority),
                "workload": format!("{:?}", snap.workload),
                "max_tokens": snap.quota.max_tokens,
                "max_syscalls": snap.quota.max_syscalls,
                "tokens_generated": snap.tokens_generated,
                "syscalls_used": snap.syscalls_used,
                "elapsed_secs": format!("{:.2}", snap.elapsed_secs).parse::<f64>().unwrap_or(snap.elapsed_secs),
            });
            protocol::response_protocol_ok(
                client,
                request_id,
                "GET_QUOTA",
                protocol::schema::GET_QUOTA,
                &json,
                Some(&json.to_string()),
            )
        } else {
            protocol::response_protocol_err_typed(
                client,
                request_id,
                ControlErrorCode::PidNotFound,
                protocol::schema::ERROR,
                &format!("PID {} not tracked by scheduler", pid),
            )
        }
    } else {
        protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::GetQuotaInvalid,
            protocol::schema::ERROR,
            "GET_QUOTA requires numeric PID",
        )
    }
}

pub(crate) fn handle_set_quota(ctx: SchedulerCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let SchedulerCommandContext {
        client,
        request_id,
        scheduler,
        pending_events,
        client_id,
    } = ctx;
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    let parts: Vec<&str> = payload_text.splitn(2, char::is_whitespace).collect();
    if parts.len() != 2 {
        protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::SetQuotaInvalid,
            protocol::schema::ERROR,
            "SET_QUOTA requires: <PID> <max_tokens=N,max_syscalls=N>",
        )
    } else if let Ok(pid) = parts[0].parse::<u64>() {
        if let Some(current) = scheduler.quota(pid).copied() {
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
                        _ => {
                            parse_ok = false;
                        }
                    }
                } else {
                    parse_ok = false;
                }
            }
            if parse_ok {
                scheduler.set_quota(pid, new_quota);
                pending_events.push(KernelEvent::WorkspaceChanged {
                    pid,
                    reason: "quota_updated".to_string(),
                });
                log_event(
                    "set_quota",
                    client_id,
                    Some(pid),
                    &format!(
                        "max_tokens={} max_syscalls={}",
                        new_quota.max_tokens, new_quota.max_syscalls
                    ),
                );
                let message = format!(
                    "PID {} quota updated: max_tokens={} max_syscalls={}",
                    pid, new_quota.max_tokens, new_quota.max_syscalls
                );
                protocol::response_protocol_ok(
                    client,
                    request_id,
                    "SET_QUOTA",
                    protocol::schema::SET_QUOTA,
                    &json!({
                        "pid": pid,
                        "max_tokens": new_quota.max_tokens,
                        "max_syscalls": new_quota.max_syscalls,
                    }),
                    Some(&message),
                )
            } else {
                protocol::response_protocol_err_typed(
                    client,
                    request_id,
                    ControlErrorCode::SetQuotaInvalid,
                    protocol::schema::ERROR,
                    "Invalid quota format. Use: max_tokens=N,max_syscalls=N",
                )
            }
        } else {
            protocol::response_protocol_err_typed(
                client,
                request_id,
                ControlErrorCode::PidNotFound,
                protocol::schema::ERROR,
                &format!("PID {} not tracked by scheduler", pid),
            )
        }
    } else {
        protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::SetQuotaInvalid,
            protocol::schema::ERROR,
            "PID must be numeric",
        )
    }
}
