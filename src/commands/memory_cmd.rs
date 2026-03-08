use crate::protocol;

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
                        protocol::response_ok_code("MEMW_QUEUED", &msg)
                    } else {
                        protocol::response_ok_code("MEMW", &msg)
                    }
                }
                Err(e) => protocol::response_err_code("MEMW_FAILED", &e.to_string()),
            }
        }
        Err(e) => protocol::response_err_code("MEMW_INVALID", &e),
    }
}
