use std::sync::atomic::Ordering;

use crate::protocol;

use super::context::CommandContext;
use super::metrics::log_event;
use super::parsing::parse_generation_payload;

pub(crate) fn handle_ping() -> Vec<u8> {
    protocol::response_ok_code("PING", "PONG")
}

pub(crate) fn handle_shutdown(ctx: &mut CommandContext<'_>) -> Vec<u8> {
    ctx.shutdown_requested.store(true, Ordering::SeqCst);
    ctx.metrics.inc_signal_count();
    log_event("kernel_shutdown", ctx.client_id, None, "shutdown_requested=true");
    protocol::response_ok_code("SHUTDOWN", "Kernel shutdown requested")
}

pub(crate) fn handle_set_gen(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if let Some(engine) = ctx.engine_state.as_mut() {
        match parse_generation_payload(&payload_text, engine.generation_config()) {
            Ok(cfg) => {
                engine.set_generation_config(cfg);
                protocol::response_ok_code(
                    "SET_GEN",
                    &format!(
                        "temperature={} top_p={} seed={} max_tokens={}",
                        cfg.temperature, cfg.top_p, cfg.seed, cfg.max_tokens
                    ),
                )
            }
            Err(e) => protocol::response_err_code("SET_GEN_INVALID", &e),
        }
    } else {
        protocol::response_err_code("NO_MODEL", "No Model Loaded")
    }
}

pub(crate) fn handle_get_gen(ctx: &mut CommandContext<'_>) -> Vec<u8> {
    if let Some(engine) = ctx.engine_state.as_ref() {
        let cfg = engine.generation_config();
        protocol::response_ok_code(
            "GET_GEN",
            &format!(
                "temperature={} top_p={} seed={} max_tokens={}",
                cfg.temperature, cfg.top_p, cfg.seed, cfg.max_tokens
            ),
        )
    } else {
        protocol::response_err_code("NO_MODEL", "No Model Loaded")
    }
}
