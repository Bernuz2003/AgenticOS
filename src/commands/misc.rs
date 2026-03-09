use std::sync::atomic::Ordering;

use serde_json::json;

use crate::protocol;

use super::context::CommandContext;
use super::metrics::log_event;
use super::parsing::parse_generation_payload;

pub(crate) fn handle_ping(ctx: &mut CommandContext<'_>) -> Vec<u8> {
    protocol::response_protocol_message(
        ctx.client,
        &ctx.request_id,
        "PING",
        protocol::schema::PING,
        "PONG",
        "PONG",
    )
}

pub(crate) fn handle_tool_info(ctx: &mut CommandContext<'_>) -> Vec<u8> {
    let cfg = crate::config::kernel_config();
    let data = json!({
        "tools": [
            {"id": "PYTHON", "description": "Execute Python code under syscall sandbox"},
            {"id": "WRITE_FILE", "description": "Write a file inside workspace"},
            {"id": "READ_FILE", "description": "Read a file inside workspace"},
            {"id": "LS", "description": "List workspace files"},
            {"id": "CALC", "description": "Evaluate numeric expressions through Python sandbox"}
        ],
        "sandbox": {
            "mode": cfg.tools.sandbox_mode,
            "allow_host_fallback": cfg.tools.allow_host_fallback,
            "timeout_s": cfg.tools.timeout_s,
            "max_calls_per_window": cfg.tools.max_calls_per_window,
            "window_s": cfg.tools.window_s,
            "error_burst_kill": cfg.tools.error_burst_kill,
        }
    });
    protocol::response_protocol_ok(
        ctx.client,
        &ctx.request_id,
        "TOOL_INFO",
        protocol::schema::TOOL_INFO,
        &data,
        Some(&data.to_string()),
    )
}

pub(crate) fn handle_shutdown(ctx: &mut CommandContext<'_>) -> Vec<u8> {
    ctx.shutdown_requested.store(true, Ordering::SeqCst);
    ctx.metrics.inc_signal_count();
    log_event("kernel_shutdown", ctx.client_id, None, "shutdown_requested=true");
    protocol::response_protocol_message(
        ctx.client,
        &ctx.request_id,
        "SHUTDOWN",
        protocol::schema::SHUTDOWN,
        "Kernel shutdown requested",
        "Kernel shutdown requested",
    )
}

pub(crate) fn handle_set_gen(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if let Some(engine) = ctx.engine_state.as_mut() {
        match parse_generation_payload(&payload_text, engine.generation_config()) {
            Ok(cfg) => {
                engine.set_generation_config(cfg);
                let message = format!(
                    "temperature={} top_p={} seed={} max_tokens={}",
                    cfg.temperature, cfg.top_p, cfg.seed, cfg.max_tokens
                );
                protocol::response_protocol_ok(
                    ctx.client,
                    &ctx.request_id,
                    "SET_GEN",
                    protocol::schema::SET_GEN,
                    &json!({
                        "temperature": cfg.temperature,
                        "top_p": cfg.top_p,
                        "seed": cfg.seed,
                        "max_tokens": cfg.max_tokens,
                    }),
                    Some(&message),
                )
            }
            Err(e) => protocol::response_protocol_err(
                ctx.client,
                &ctx.request_id,
                "SET_GEN_INVALID",
                protocol::schema::ERROR,
                &e,
            ),
        }
    } else {
        protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "NO_MODEL",
            protocol::schema::ERROR,
            "No Model Loaded",
        )
    }
}

pub(crate) fn handle_get_gen(ctx: &mut CommandContext<'_>) -> Vec<u8> {
    if let Some(engine) = ctx.engine_state.as_ref() {
        let cfg = engine.generation_config();
        let message = format!(
            "temperature={} top_p={} seed={} max_tokens={}",
            cfg.temperature, cfg.top_p, cfg.seed, cfg.max_tokens
        );
        protocol::response_protocol_ok(
            ctx.client,
            &ctx.request_id,
            "GET_GEN",
            protocol::schema::GET_GEN,
            &json!({
                "temperature": cfg.temperature,
                "top_p": cfg.top_p,
                "seed": cfg.seed,
                "max_tokens": cfg.max_tokens,
            }),
            Some(&message),
        )
    } else {
        protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "NO_MODEL",
            protocol::schema::ERROR,
            "No Model Loaded",
        )
    }
}
