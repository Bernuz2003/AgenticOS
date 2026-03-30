use crate::diagnostics::audit::{self, AuditContext};
use crate::engine::LLMEngine;
use crate::orchestrator::{Orchestrator, TaskPidBinding, TaskStatus};
use crate::session::SessionRegistry;
use crate::storage::{IpcMailboxSelector, NewIpcMessage, StorageService, StoredIpcMessage};

use super::ids::next_ipc_message_id;

pub(super) fn non_empty_input_str<'a>(input: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn build_ipc_payload_preview(payload_text: &str) -> String {
    if payload_text.chars().count() > 240 {
        let mut preview = payload_text.chars().take(240).collect::<String>();
        preview.push_str("...");
        preview
    } else {
        payload_text.to_string()
    }
}

fn task_role_for_binding(orchestrator: &Orchestrator, binding: &TaskPidBinding) -> Option<String> {
    orchestrator
        .get(binding.orch_id)
        .and_then(|orch| orch.tasks.get(&binding.task_id))
        .and_then(|task| task.role.clone())
        .map(|role| role.trim().to_string())
        .filter(|role| !role.is_empty())
}

fn resolve_named_task_target(
    orchestrator: &Orchestrator,
    orch_id: u64,
    task_id: &str,
) -> Option<(Option<u64>, Option<u32>, Option<String>)> {
    let orch = orchestrator.get(orch_id)?;
    let task = orch.tasks.get(task_id)?;
    let role = task
        .role
        .clone()
        .map(|role| role.trim().to_string())
        .filter(|role| !role.is_empty());
    let attempt = match orch.status.get(task_id) {
        Some(TaskStatus::Running { pid, attempt }) => {
            return Some((Some(*pid), Some(*attempt), role));
        }
        Some(TaskStatus::Completed { attempt }) | Some(TaskStatus::Failed { attempt, .. }) => {
            Some(*attempt)
        }
        Some(TaskStatus::Pending | TaskStatus::Skipped) => None,
        None => return None,
    };
    Some((None, attempt, role))
}

fn resolve_role_target(
    orchestrator: &Orchestrator,
    orch_id: u64,
    role: &str,
) -> Option<(String, Option<u64>, Option<u32>)> {
    let orch = orchestrator.get(orch_id)?;
    let target_role = role.trim();
    if target_role.is_empty() {
        return None;
    }

    let mut fallback: Option<(String, Option<u64>, Option<u32>)> = None;
    for task_id in &orch.topo_order {
        let Some(task) = orch.tasks.get(task_id) else {
            continue;
        };
        let matches_role = task
            .role
            .as_deref()
            .map(str::trim)
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(target_role));
        if !matches_role {
            continue;
        }

        match orch.status.get(task_id) {
            Some(TaskStatus::Running { pid, attempt }) => {
                return Some((task_id.clone(), Some(*pid), Some(*attempt)));
            }
            Some(TaskStatus::Completed { attempt }) | Some(TaskStatus::Failed { attempt, .. }) => {
                fallback.get_or_insert((task_id.clone(), None, Some(*attempt)));
            }
            Some(TaskStatus::Pending | TaskStatus::Skipped) => {
                fallback.get_or_insert((task_id.clone(), None, None));
            }
            None => {}
        }
    }

    fallback
}

fn format_ipc_sender_label(message: &StoredIpcMessage) -> String {
    message
        .sender_task_id
        .clone()
        .or_else(|| message.sender_pid.map(|pid| format!("pid {pid}")))
        .unwrap_or_else(|| "unknown sender".to_string())
}

fn format_ipc_receiver_label(message: &StoredIpcMessage) -> String {
    message
        .receiver_task_id
        .clone()
        .or_else(|| {
            message
                .receiver_role
                .clone()
                .map(|role| format!("role:{role}"))
        })
        .or_else(|| {
            message
                .channel
                .clone()
                .map(|channel| format!("channel:{channel}"))
        })
        .or_else(|| message.receiver_pid.map(|pid| format!("pid {pid}")))
        .unwrap_or_else(|| "unknown target".to_string())
}

fn channel_mailbox_match(selector: &IpcMailboxSelector, message: &StoredIpcMessage) -> bool {
    selector.orchestration_id.is_some()
        && message.orchestration_id == selector.orchestration_id
        && message.channel.is_some()
        && message.receiver_pid.is_none()
        && message.receiver_task_id.is_none()
        && message.receiver_role.is_none()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_send_action(
    runtime_id: &str,
    engine: &mut LLMEngine,
    orchestrator: &Orchestrator,
    pid: u64,
    target_pid: Option<u64>,
    target_task: Option<&str>,
    target_role: Option<&str>,
    explicit_orchestration_id: Option<u64>,
    message: Option<&str>,
    payload: Option<&serde_json::Value>,
    message_type: &str,
    channel: Option<&str>,
    session_registry: &SessionRegistry,
    storage: &mut StorageService,
) {
    let normalized_message = message.map(str::trim).filter(|value| !value.is_empty());
    let payload_text = normalized_message
        .map(ToString::to_string)
        .or_else(|| payload.map(|value| value.to_string()))
        .unwrap_or_default();
    if payload_text.is_empty() {
        audit::record(
            storage,
            audit::ACTION_DENIED,
            "action=send reason=empty_payload",
            AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(runtime_id),
            ),
        );
        let _ = engine.inject_context(
            pid,
            &engine.format_system_message(
                "ACTION Error: ACTION:send requires a non-empty message or payload.",
            ),
        );
        return;
    }

    let sender_binding = orchestrator.task_binding_for_pid(pid);
    let sender_role = sender_binding
        .as_ref()
        .and_then(|binding| task_role_for_binding(orchestrator, binding));
    let mut orchestration_id = explicit_orchestration_id
        .or_else(|| sender_binding.as_ref().map(|binding| binding.orch_id));
    let mut receiver_pid = target_pid;
    let mut receiver_task_id = target_task.map(ToString::to_string);
    let mut receiver_attempt = None;
    let mut receiver_role = target_role.map(ToString::to_string);

    if orchestration_id.is_none() {
        orchestration_id = receiver_pid
            .and_then(|candidate_pid| orchestrator.task_binding_for_pid(candidate_pid))
            .map(|binding| binding.orch_id);
    }

    if let Some(task_id) = target_task {
        let Some(orch_id) = orchestration_id else {
            audit::record(
                storage,
                audit::ACTION_DENIED,
                "action=send reason=missing_orchestration_for_task_target",
                AuditContext::for_process(
                    session_registry.session_id_for_pid(pid),
                    pid,
                    Some(runtime_id),
                ),
            );
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(
                    "ACTION Error: task-targeted IPC requires an orchestration context.",
                ),
            );
            return;
        };
        let Some((resolved_pid, resolved_attempt, resolved_role)) =
            resolve_named_task_target(orchestrator, orch_id, task_id)
        else {
            audit::record(
                storage,
                audit::ACTION_DENIED,
                format!("action=send reason=unknown_task_target task={task_id} orch_id={orch_id}"),
                AuditContext::for_process(
                    session_registry.session_id_for_pid(pid),
                    pid,
                    Some(runtime_id),
                ),
            );
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(&format!(
                    "ACTION Error: workflow task '{task_id}' not found in orchestration {orch_id}.",
                )),
            );
            return;
        };
        receiver_pid = receiver_pid.or(resolved_pid);
        receiver_attempt = resolved_attempt;
        receiver_role = receiver_role.or(resolved_role);
    }

    if let Some(role) = target_role {
        let Some(orch_id) = orchestration_id else {
            audit::record(
                storage,
                audit::ACTION_DENIED,
                "action=send reason=missing_orchestration_for_role_target",
                AuditContext::for_process(
                    session_registry.session_id_for_pid(pid),
                    pid,
                    Some(runtime_id),
                ),
            );
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(
                    "ACTION Error: role-targeted IPC requires an orchestration context.",
                ),
            );
            return;
        };
        let Some((resolved_task_id, resolved_pid, resolved_attempt)) =
            resolve_role_target(orchestrator, orch_id, role)
        else {
            audit::record(
                storage,
                audit::ACTION_DENIED,
                format!("action=send reason=unknown_role_target role={role} orch_id={orch_id}"),
                AuditContext::for_process(
                    session_registry.session_id_for_pid(pid),
                    pid,
                    Some(runtime_id),
                ),
            );
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(&format!(
                    "ACTION Error: workflow role '{role}' not found in orchestration {orch_id}.",
                )),
            );
            return;
        };
        receiver_task_id = receiver_task_id.or(Some(resolved_task_id));
        receiver_pid = receiver_pid.or(resolved_pid);
        receiver_attempt = receiver_attempt.or(resolved_attempt);
    }

    let receiver_binding = receiver_pid.and_then(|candidate_pid| {
        let binding = orchestrator.task_binding_for_pid(candidate_pid)?;
        Some(binding)
    });
    if receiver_task_id.is_none() {
        receiver_task_id = receiver_binding
            .as_ref()
            .map(|binding| binding.task_id.clone());
    }
    if receiver_attempt.is_none() {
        receiver_attempt = receiver_binding.as_ref().map(|binding| binding.attempt);
    }
    if receiver_role.is_none() {
        receiver_role = receiver_binding
            .as_ref()
            .and_then(|binding| task_role_for_binding(orchestrator, binding));
    }

    let direct_pid_delivery =
        target_pid.is_some() && target_task.is_none() && target_role.is_none() && channel.is_none();
    let message_id = next_ipc_message_id();
    let preview = build_ipc_payload_preview(&payload_text);
    let _ = storage.record_ipc_message(&NewIpcMessage {
        message_id: message_id.clone(),
        orchestration_id,
        sender_pid: Some(pid),
        sender_task_id: sender_binding
            .as_ref()
            .map(|binding| binding.task_id.clone()),
        sender_attempt: sender_binding.as_ref().map(|binding| binding.attempt),
        receiver_pid,
        receiver_task_id: receiver_task_id.clone(),
        receiver_attempt,
        receiver_role: receiver_role.clone(),
        message_type: message_type.to_string(),
        channel: channel.map(ToString::to_string),
        payload_preview: preview.clone(),
        payload_text: payload_text.clone(),
        status: "queued".to_string(),
    });

    if direct_pid_delivery {
        let Some(target_pid) = target_pid else {
            return;
        };
        let msg_target = engine.format_interprocess_message(pid, &payload_text);
        match engine.inject_context(target_pid, &msg_target) {
            Ok(_) => {
                let _ = storage.update_ipc_message_status(
                    &message_id,
                    "delivered",
                    Some(crate::storage::current_timestamp_ms()),
                    None,
                    None,
                );
                audit::record(
                    storage,
                    audit::ACTION_COMPLETED,
                    format!(
                        "action=send message_id={} type={} target=pid:{} chars={} delivery=direct",
                        message_id,
                        message_type,
                        target_pid,
                        payload_text.chars().count()
                    ),
                    AuditContext::for_process(
                        session_registry.session_id_for_pid(pid),
                        pid,
                        Some(runtime_id),
                    ),
                );
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(
                        "MESSAGE DELIVERED. The target PID received it immediately.",
                    ),
                );
            }
            Err(_) => {
                let _ = storage.update_ipc_message_status(
                    &message_id,
                    "failed",
                    None,
                    None,
                    Some(crate::storage::current_timestamp_ms()),
                );
                audit::record(
                    storage,
                    audit::ACTION_DENIED,
                    format!(
                        "action=send message_id={} type={} reason=target_not_found target_pid={}",
                        message_id, message_type, target_pid
                    ),
                    AuditContext::for_process(
                        session_registry.session_id_for_pid(pid),
                        pid,
                        Some(runtime_id),
                    ),
                );
                let _ = engine.inject_context(
                    pid,
                    &engine.format_system_message(
                        "ACTION Error: target PID not found for direct IPC delivery.",
                    ),
                );
            }
        }
        return;
    }

    let target_summary = receiver_task_id
        .clone()
        .or_else(|| receiver_role.clone().map(|role| format!("role:{role}")))
        .or_else(|| channel.map(|channel| format!("channel:{channel}")))
        .or_else(|| receiver_pid.map(|candidate_pid| format!("pid:{candidate_pid}")))
        .unwrap_or_else(|| "mailbox".to_string());
    audit::record(
        storage,
        audit::ACTION_COMPLETED,
        format!(
            "action=send message_id={} type={} target={} chars={} delivery=queued sender_role={}",
            message_id,
            message_type,
            target_summary,
            payload_text.chars().count(),
            sender_role.unwrap_or_else(|| "-".to_string())
        ),
        AuditContext::for_process(
            session_registry.session_id_for_pid(pid),
            pid,
            Some(runtime_id),
        ),
    );
    let _ = engine.inject_context(
        pid,
        &engine.format_system_message(&format!(
            "MESSAGE QUEUED. message_id={} target={} status=queued. The receiver should use ACTION:receive and ACTION:ack.",
            message_id, target_summary
        )),
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_receive_action(
    runtime_id: &str,
    engine: &mut LLMEngine,
    orchestrator: &Orchestrator,
    pid: u64,
    limit: usize,
    channel: Option<&str>,
    include_delivered: bool,
    session_registry: &SessionRegistry,
    storage: &mut StorageService,
) {
    let binding = orchestrator.task_binding_for_pid(pid);
    if channel.is_some() && binding.is_none() {
        let _ = engine.inject_context(
            pid,
            &engine.format_system_message(
                "ACTION Error: channel mailbox reads require a workflow task context.",
            ),
        );
        return;
    }

    let selector = IpcMailboxSelector {
        orchestration_id: binding.as_ref().map(|entry| entry.orch_id),
        receiver_pid: Some(pid),
        receiver_task_id: binding.as_ref().map(|entry| entry.task_id.clone()),
        receiver_role: binding
            .as_ref()
            .and_then(|entry| task_role_for_binding(orchestrator, entry)),
        channel: channel.map(ToString::to_string),
    };
    let messages = match storage.load_ipc_mailbox_messages(&selector, include_delivered, limit) {
        Ok(messages) => messages,
        Err(err) => {
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(&format!(
                    "ACTION Error: failed to read mailbox messages: {err}",
                )),
            );
            return;
        }
    };

    if messages.is_empty() {
        audit::record(
            storage,
            audit::ACTION_COMPLETED,
            format!("action=receive count=0 channel={}", channel.unwrap_or("-")),
            AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(runtime_id),
            ),
        );
        let _ = engine.inject_context(
            pid,
            &engine.format_system_message("MAILBOX EMPTY. No pending IPC messages matched."),
        );
        return;
    }

    let delivered_at_ms = crate::storage::current_timestamp_ms();
    for message in &messages {
        if matches!(message.status.as_str(), "queued" | "pending") {
            let _ = storage.update_ipc_message_status(
                &message.message_id,
                "delivered",
                Some(delivered_at_ms),
                None,
                None,
            );
        }
    }

    let mut rendered = format!(
        "MAILBOX RECEIVED {} message(s).\nUse ACTION:ack once you have processed them.\n",
        messages.len()
    );
    for message in &messages {
        rendered.push_str(&format!(
            "\n[message_id={} type={} status={} from={} to={}{}]\n{}\n",
            message.message_id,
            message.message_type,
            if matches!(message.status.as_str(), "queued" | "pending") {
                "delivered"
            } else {
                message.status.as_str()
            },
            format_ipc_sender_label(message),
            format_ipc_receiver_label(message),
            message
                .channel
                .as_ref()
                .map(|value| format!(" channel={value}"))
                .unwrap_or_default(),
            message.payload_text
        ));
    }

    audit::record(
        storage,
        audit::ACTION_COMPLETED,
        format!(
            "action=receive count={} channel={} include_delivered={}",
            messages.len(),
            channel.unwrap_or("-"),
            include_delivered
        ),
        AuditContext::for_process(
            session_registry.session_id_for_pid(pid),
            pid,
            Some(runtime_id),
        ),
    );
    let _ = engine.inject_context(pid, &engine.format_system_message(&rendered));
}

pub(super) fn dispatch_ack_action(
    runtime_id: &str,
    engine: &mut LLMEngine,
    orchestrator: &Orchestrator,
    pid: u64,
    message_ids: &[String],
    session_registry: &SessionRegistry,
    storage: &mut StorageService,
) {
    let binding = orchestrator.task_binding_for_pid(pid);
    let selector = IpcMailboxSelector {
        orchestration_id: binding.as_ref().map(|entry| entry.orch_id),
        receiver_pid: Some(pid),
        receiver_task_id: binding.as_ref().map(|entry| entry.task_id.clone()),
        receiver_role: binding
            .as_ref()
            .and_then(|entry| task_role_for_binding(orchestrator, entry)),
        channel: None,
    };
    let loaded = match storage.load_ipc_messages_by_ids(message_ids) {
        Ok(messages) => messages,
        Err(err) => {
            let _ = engine.inject_context(
                pid,
                &engine.format_system_message(&format!(
                    "ACTION Error: failed to load IPC messages for ack: {err}",
                )),
            );
            return;
        }
    };
    let eligible = loaded
        .into_iter()
        .filter(|message| {
            matches!(message.status.as_str(), "queued" | "pending" | "delivered")
                && (selector.matches(message) || channel_mailbox_match(&selector, message))
        })
        .collect::<Vec<_>>();

    if eligible.is_empty() {
        audit::record(
            storage,
            audit::ACTION_COMPLETED,
            "action=ack count=0",
            AuditContext::for_process(
                session_registry.session_id_for_pid(pid),
                pid,
                Some(runtime_id),
            ),
        );
        let _ = engine.inject_context(
            pid,
            &engine.format_system_message(
                "MAILBOX ACK skipped: no matching messages were eligible for this process mailbox.",
            ),
        );
        return;
    }

    let consumed_at_ms = crate::storage::current_timestamp_ms();
    let acked_ids = eligible
        .iter()
        .map(|message| message.message_id.clone())
        .collect::<Vec<_>>();
    for message_id in &acked_ids {
        let _ = storage.update_ipc_message_status(
            message_id,
            "consumed",
            None,
            Some(consumed_at_ms),
            None,
        );
    }

    audit::record(
        storage,
        audit::ACTION_COMPLETED,
        format!("action=ack count={}", acked_ids.len()),
        AuditContext::for_process(
            session_registry.session_id_for_pid(pid),
            pid,
            Some(runtime_id),
        ),
    );
    let _ = engine.inject_context(
        pid,
        &engine.format_system_message(&format!(
            "MAILBOX ACK completed for {} message(s): {}",
            acked_ids.len(),
            acked_ids.join(", ")
        )),
    );
}
