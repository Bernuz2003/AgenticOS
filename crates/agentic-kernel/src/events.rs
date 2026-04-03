use std::collections::HashMap;

use agentic_control_models::{KernelEvent, KernelEventEnvelope};
use mio::{Poll, Token};

use crate::protocol;
use crate::runtime::{AssistantTurnRuntimeBoundary, TurnAssemblyStore};
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::transport::{writable_interest, Client};

pub fn flush_pending_events(
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    next_sequence: &mut u64,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    turn_assembly: &mut TurnAssemblyStore,
    pending_events: &mut Vec<KernelEvent>,
) {
    if pending_events.is_empty() {
        return;
    }

    for event in pending_events.drain(..) {
        if let Err(err) = persist_event(session_registry, storage, turn_assembly, &event) {
            tracing::error!(%err, "EVENTS: failed to persist kernel event");
        }
        *next_sequence = next_sequence.saturating_add(1);
        let envelope = KernelEventEnvelope {
            seq: *next_sequence,
            event,
        };
        let payload = match serde_json::to_vec(&envelope) {
            Ok(payload) => payload,
            Err(err) => {
                tracing::error!(%err, "EVENTS: failed to serialize kernel event");
                continue;
            }
        };
        let frame = protocol::response_data_with_code("event", &payload);

        for (token, client) in clients.iter_mut() {
            if !client.subscribed_events {
                continue;
            }

            client.output_buffer.extend(frame.iter().copied());
            let _ = poll
                .registry()
                .reregister(&mut client.stream, *token, writable_interest());
        }
    }
}

fn persist_event(
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    turn_assembly: &mut TurnAssemblyStore,
    event: &KernelEvent,
) -> Result<(), crate::storage::StorageError> {
    match event {
        KernelEvent::SessionStarted {
            session_id,
            pid,
            workload,
            prompt,
        } => {
            let turn_id = storage.start_session_turn(
                session_id,
                *pid,
                workload,
                "kernel_event",
                prompt,
                "prompt",
            )?;
            turn_assembly.apply_runtime_boundary(*pid, AssistantTurnRuntimeBoundary::RuntimeClosed);
            session_registry.remember_active_turn(*pid, turn_id);
            Ok(())
        }
        KernelEvent::TimelineSegment { .. } => Ok(()),
        KernelEvent::InvocationUpdated { pid, invocation } => {
            let Some(turn_id) = session_registry.active_turn_id_for_pid(*pid) else {
                return Ok(());
            };
            flush_pending_assistant_segments(storage, session_registry, turn_assembly, *pid)?;
            let payload = serde_json::to_string(invocation).map_err(|err| {
                crate::storage::StorageError::Sqlite(rusqlite::Error::ToSqlConversionFailure(
                    Box::new(err),
                ))
            })?;
            storage.append_system_message(turn_id, "invocation", &payload)
        }
        KernelEvent::SessionFinished {
            pid,
            tokens_generated,
            elapsed_secs,
            reason,
        } => {
            let Some(turn_id) = session_registry.active_turn_id_for_pid(*pid) else {
                return Ok(());
            };
            flush_pending_assistant_segments(storage, session_registry, turn_assembly, *pid)?;
            if reason == "awaiting_turn_decision" {
                let result = storage.mark_turn_awaiting_decision(turn_id);
                if result.is_ok() {
                    turn_assembly
                        .apply_runtime_boundary(*pid, AssistantTurnRuntimeBoundary::RuntimeClosed);
                }
                return result;
            }
            let (status, marker_text) =
                finish_reason_to_turn_outcome(reason, *tokens_generated, *elapsed_secs);
            let result = storage.finish_turn(turn_id, status, reason, marker_text.as_deref());
            if result.is_ok() {
                session_registry.clear_active_turn(*pid);
                turn_assembly
                    .apply_runtime_boundary(*pid, AssistantTurnRuntimeBoundary::RuntimeClosed);
            }
            result
        }
        KernelEvent::SessionErrored { pid, message } => {
            let Some(turn_id) = session_registry.active_turn_id_for_pid(*pid) else {
                return Ok(());
            };
            flush_pending_assistant_segments(storage, session_registry, turn_assembly, *pid)?;
            let result = storage.error_turn(turn_id, message);
            if result.is_ok() {
                session_registry.clear_active_turn(*pid);
                turn_assembly
                    .apply_runtime_boundary(*pid, AssistantTurnRuntimeBoundary::RuntimeClosed);
            }
            result
        }
        _ => Ok(()),
    }
}

fn flush_pending_assistant_segments(
    storage: &mut StorageService,
    session_registry: &mut SessionRegistry,
    turn_assembly: &mut TurnAssemblyStore,
    pid: u64,
) -> Result<(), crate::storage::StorageError> {
    let Some(turn_id) = session_registry.active_turn_id_for_pid(pid) else {
        return Ok(());
    };
    let Some(segments) = turn_assembly.drain_pending_segments(pid) else {
        return Ok(());
    };
    for segment in segments {
        storage.append_assistant_segment(turn_id, segment.kind, &segment.text)?;
    }
    Ok(())
}

fn finish_reason_to_turn_outcome(
    reason: &str,
    tokens_generated: Option<u64>,
    elapsed_secs: Option<f64>,
) -> (&'static str, Option<String>) {
    match reason {
        "turn_completed" => ("completed", None),
        "output_stopped" => ("completed", None),
        "completed" => (
            "completed",
            tokens_generated
                .zip(elapsed_secs)
                .map(|(tokens_generated, elapsed_secs)| {
                    format!(
                        "Process finished: tokens_generated={} elapsed_secs={:.3}",
                        tokens_generated, elapsed_secs
                    )
                }),
        ),
        "terminated" => ("terminated", Some("terminated".to_string())),
        "killed" => ("killed", Some("killed".to_string())),
        "orchestrator_killed" => ("killed", Some("orchestrator_killed".to_string())),
        "syscall_killed" => ("killed", Some("syscall_killed".to_string())),
        "worker_error" => ("errored", Some("worker_error".to_string())),
        other => ("completed", Some(other.to_string())),
    }
}
