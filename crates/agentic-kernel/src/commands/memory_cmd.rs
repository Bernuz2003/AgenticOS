use crate::protocol;
use agentic_control_models::KernelEvent;
use agentic_protocol::ControlErrorCode;
use serde_json::json;

use super::context::MemoryCommandContext;
use super::parsing::parse_memw_payload;

pub(crate) fn handle_memory_write(ctx: MemoryCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let MemoryCommandContext {
        client,
        request_id,
        memory,
        engine_state,
        pending_events,
    } = ctx;
    match parse_memw_payload(payload) {
        Ok((pid, raw)) => {
            let backend_id = engine_state
                .as_ref()
                .map(|engine| engine.loaded_backend_id())
                .or(Some("external-llamacpp"));
            match memory.write_for_pid_bytes_with_backend(pid, &raw, backend_id) {
                Ok(msg) => {
                    let is_parked = memory.is_pid_parked(pid);
                    pending_events.push(KernelEvent::WorkspaceChanged {
                        pid,
                        reason: if is_parked {
                            "memory_queued".to_string()
                        } else {
                            "memory_written".to_string()
                        },
                    });
                    pending_events.push(KernelEvent::LobbyChanged {
                        reason: "memory_updated".to_string(),
                    });

                    if is_parked {
                        if let Some(engine) = engine_state.as_mut() {
                            let _ = engine.park_process(pid);
                        }
                        protocol::response_protocol_ok(
                            client,
                            request_id,
                            "MEMW_QUEUED",
                            protocol::schema::MEMORY_WRITE,
                            &json!({"pid": pid, "status": "queued", "message": msg}),
                            Some(&msg),
                        )
                    } else {
                        protocol::response_protocol_ok(
                            client,
                            request_id,
                            "MEMW",
                            protocol::schema::MEMORY_WRITE,
                            &json!({"pid": pid, "status": "written", "message": msg}),
                            Some(&msg),
                        )
                    }
                }
                Err(e) => protocol::response_protocol_err_typed(
                    client,
                    request_id,
                    ControlErrorCode::MemwFailed,
                    protocol::schema::ERROR,
                    &e.to_string(),
                ),
            }
        }
        Err(e) => protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::MemwInvalid,
            protocol::schema::ERROR,
            &e,
        ),
    }
}
