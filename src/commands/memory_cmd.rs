use crate::protocol;

use super::context::CommandContext;
use super::parsing::parse_memw_payload;

pub(crate) fn handle_memory_write(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    match parse_memw_payload(payload) {
        Ok((pid, raw)) => {
            let mut mem = ctx.memory.borrow_mut();
            match mem.write_for_pid_bytes(pid, &raw) {
                Ok(msg) => {
                    let is_waiting = mem.is_pid_waiting_for_memory(pid);
                    drop(mem);

                    if is_waiting {
                        let mut lock = ctx.engine_state.lock().expect("engine_state lock poisoned");
                        if let Some(engine) = lock.as_mut() {
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
