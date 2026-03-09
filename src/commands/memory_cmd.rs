use crate::protocol;
use serde_json::json;

use super::context::CommandContext;
use super::parsing::parse_memw_payload;

pub(crate) fn handle_memory_write(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    match parse_memw_payload(payload) {
        Ok((pid, raw)) => {
            let backend_id = ctx
                .engine_state
                .as_ref()
                .map(|engine| engine.loaded_backend_id())
                .or(Some("candle.slot-compat"));
            match ctx.memory.write_for_pid_bytes_with_backend(pid, &raw, backend_id) {
                Ok(msg) => {
                    let is_waiting = ctx.memory.is_pid_waiting_for_memory(pid);

                    if is_waiting {
                        if let Some(engine) = ctx.engine_state.as_mut() {
                            let _ = engine.set_process_waiting_for_memory(pid);
                        }
                        protocol::response_protocol_ok(
                            ctx.client,
                            &ctx.request_id,
                            "MEMW_QUEUED",
                            protocol::schema::MEMORY_WRITE,
                            &json!({"pid": pid, "status": "queued", "message": msg}),
                            Some(&msg),
                        )
                    } else {
                        protocol::response_protocol_ok(
                            ctx.client,
                            &ctx.request_id,
                            "MEMW",
                            protocol::schema::MEMORY_WRITE,
                            &json!({"pid": pid, "status": "written", "message": msg}),
                            Some(&msg),
                        )
                    }
                }
                Err(e) => protocol::response_protocol_err(
                    ctx.client,
                    &ctx.request_id,
                    "MEMW_FAILED",
                    protocol::schema::ERROR,
                    &e.to_string(),
                ),
            }
        }
        Err(e) => protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "MEMW_INVALID",
            protocol::schema::ERROR,
            &e,
        ),
    }
}
