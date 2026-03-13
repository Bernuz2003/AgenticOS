use std::sync::atomic::Ordering;

use agentic_control_models::{KernelEvent, SubscribeResult};
use agentic_protocol::ControlErrorCode;
use serde_json::json;

use crate::protocol;

use super::context::MiscCommandContext;
use super::metrics::log_event;
use super::parsing::parse_generation_payload;

pub(crate) fn handle_ping(ctx: MiscCommandContext<'_>) -> Vec<u8> {
    protocol::response_protocol_message(
        ctx.client,
        ctx.request_id,
        "PING",
        protocol::schema::PING,
        "PONG",
        "PONG",
    )
}

pub(crate) fn handle_subscribe(ctx: MiscCommandContext<'_>) -> Vec<u8> {
    let MiscCommandContext {
        client, request_id, ..
    } = ctx;
    if !client.enabled_capabilities.contains("event_stream_v1") {
        return protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::CapabilityRequired,
            protocol::schema::ERROR,
            "SUBSCRIBE requires HELLO negotiation with capability 'event_stream_v1'.",
        );
    }

    client.subscribed_events = true;
    protocol::response_protocol_ok(
        client,
        request_id,
        "SUBSCRIBE",
        protocol::schema::SUBSCRIBE,
        &SubscribeResult {
            scope: "kernel_runtime".to_string(),
        },
        Some("{\"scope\":\"kernel_runtime\"}"),
    )
}

pub(crate) fn handle_shutdown(ctx: MiscCommandContext<'_>) -> Vec<u8> {
    let MiscCommandContext {
        client,
        request_id,
        shutdown_requested,
        pending_events,
        metrics,
        client_id,
        ..
    } = ctx;
    shutdown_requested.store(true, Ordering::SeqCst);
    metrics.inc_signal_count();
    pending_events.push(KernelEvent::KernelShutdownRequested);
    log_event(
        "kernel_shutdown",
        client_id,
        None,
        "shutdown_requested=true",
    );
    protocol::response_protocol_message(
        client,
        request_id,
        "SHUTDOWN",
        protocol::schema::SHUTDOWN,
        "Kernel shutdown requested",
        "Kernel shutdown requested",
    )
}

pub(crate) fn handle_set_gen(ctx: MiscCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let MiscCommandContext {
        client,
        request_id,
        runtime_registry,
        ..
    } = ctx;
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    if let Some(engine) = runtime_registry.current_engine_mut() {
        match parse_generation_payload(&payload_text, engine.generation_config()) {
            Ok(cfg) => {
                engine.set_generation_config(cfg);
                let message = format!(
                    "temperature={} top_p={} seed={} max_tokens={}",
                    cfg.temperature, cfg.top_p, cfg.seed, cfg.max_tokens
                );
                protocol::response_protocol_ok(
                    client,
                    request_id,
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
            Err(e) => protocol::response_protocol_err_typed(
                client,
                request_id,
                ControlErrorCode::SetGenInvalid,
                protocol::schema::ERROR,
                &e,
            ),
        }
    } else {
        protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            "No Model Loaded",
        )
    }
}

pub(crate) fn handle_get_gen(ctx: MiscCommandContext<'_>) -> Vec<u8> {
    let MiscCommandContext {
        client,
        request_id,
        runtime_registry,
        ..
    } = ctx;
    if let Some(engine) = runtime_registry.current_engine() {
        let cfg = engine.generation_config();
        let message = format!(
            "temperature={} top_p={} seed={} max_tokens={}",
            cfg.temperature, cfg.top_p, cfg.seed, cfg.max_tokens
        );
        protocol::response_protocol_ok(
            client,
            request_id,
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
        protocol::response_protocol_err_typed(
            client,
            request_id,
            ControlErrorCode::NoModel,
            protocol::schema::ERROR,
            "No Model Loaded",
        )
    }
}
