use serde::Deserialize;
use serde_json::json;

use crate::config::kernel_config;
use crate::protocol;
use crate::tool_registry::{ToolRegistryEntry, ToolSource};

use super::context::CommandContext;

#[derive(Debug, Deserialize)]
struct RegisterToolRequest {
    descriptor: crate::tool_registry::ToolDescriptor,
    backend: crate::tool_registry::ToolBackendConfig,
}

#[derive(Debug, Deserialize)]
struct UnregisterToolRequest {
    name: String,
}

pub(crate) fn handle_list_tools(ctx: &mut CommandContext<'_>) -> Vec<u8> {
    let tools: Vec<serde_json::Value> = ctx
        .tool_registry
        .list()
        .into_iter()
        .map(|tool| serde_json::to_value(tool).expect("tool descriptor serializable"))
        .collect();

    let payload = json!({
        "total_tools": tools.len(),
        "tools": tools,
    });

    protocol::response_protocol_ok(
        ctx.client,
        &ctx.request_id,
        "LIST_TOOLS",
        protocol::schema::LIST_TOOLS,
        &payload,
        Some(&payload.to_string()),
    )
}

pub(crate) fn handle_tool_info(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let tool_name = String::from_utf8_lossy(payload).trim().to_string();
    if tool_name.is_empty() {
        return protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "MISSING_TOOL_NAME",
            protocol::schema::ERROR,
            "TOOL_INFO requires a tool name payload",
        );
    }

    let Some(tool) = ctx.tool_registry.get(&tool_name) else {
        return protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "TOOL_NOT_FOUND",
            protocol::schema::ERROR,
            &format!("Tool '{}' is not registered.", tool_name),
        );
    };

    let cfg = crate::config::kernel_config();
    let payload = json!({
        "tool": tool,
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
        &payload,
        Some(&payload.to_string()),
    )
}

pub(crate) fn handle_register_tool(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    if let Some(response) = ensure_registry_mutation_allowed(ctx, "REGISTER_TOOL") {
        return response;
    }

    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    let mut request = match serde_json::from_str::<RegisterToolRequest>(&payload_text) {
        Ok(request) => request,
        Err(err) => {
            return protocol::response_protocol_err(
                ctx.client,
                &ctx.request_id,
                "INVALID_TOOL_REGISTRATION",
                protocol::schema::ERROR,
                &format!("REGISTER_TOOL payload must be valid JSON: {}", err),
            );
        }
    };

    request.descriptor.source = ToolSource::Runtime;
    let requested_name = request.descriptor.name.clone();
    let entry = ToolRegistryEntry {
        descriptor: request.descriptor,
        backend: request.backend,
    };
    if entry.descriptor.name.trim().is_empty() {
        return protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "INVALID_TOOL_NAME",
            protocol::schema::ERROR,
            "REGISTER_TOOL requires a non-empty descriptor.name",
        );
    }

    match ctx.tool_registry.register(entry) {
        Ok(()) => {
            let registered = ctx
                .tool_registry
                .get(&requested_name)
                .expect("registered tool must be retrievable");
            let payload = json!({
                "tool": registered,
            });
            protocol::response_protocol_ok(
                ctx.client,
                &ctx.request_id,
                "REGISTER_TOOL",
                protocol::schema::REGISTER_TOOL,
                &payload,
                Some(&payload.to_string()),
            )
        }
        Err(err) => protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "REGISTER_TOOL_FAILED",
            protocol::schema::ERROR,
            &err,
        ),
    }
}

pub(crate) fn handle_unregister_tool(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    if let Some(response) = ensure_registry_mutation_allowed(ctx, "UNREGISTER_TOOL") {
        return response;
    }

    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    let request = match serde_json::from_str::<UnregisterToolRequest>(&payload_text) {
        Ok(request) => request,
        Err(err) => {
            return protocol::response_protocol_err(
                ctx.client,
                &ctx.request_id,
                "INVALID_TOOL_UNREGISTRATION",
                protocol::schema::ERROR,
                &format!("UNREGISTER_TOOL payload must be valid JSON: {}", err),
            );
        }
    };

    match ctx.tool_registry.unregister(&request.name) {
        Ok(removed) => {
            let payload = json!({
                "tool": removed,
            });
            protocol::response_protocol_ok(
                ctx.client,
                &ctx.request_id,
                "UNREGISTER_TOOL",
                protocol::schema::UNREGISTER_TOOL,
                &payload,
                Some(&payload.to_string()),
            )
        }
        Err(err) => protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "UNREGISTER_TOOL_FAILED",
            protocol::schema::ERROR,
            &err,
        ),
    }
}

fn ensure_registry_mutation_allowed(ctx: &mut CommandContext<'_>, opcode: &str) -> Option<Vec<u8>> {
    if kernel_config().auth.disabled {
        return Some(protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "TOOL_REGISTRY_MUTATION_FORBIDDEN",
            protocol::schema::ERROR,
            &format!("{} requires auth.enabled so the kernel can establish a privileged session.", opcode),
        ));
    }

    if !ctx.client.enabled_capabilities.contains("tool_registry_v1") {
        return Some(protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "CAPABILITY_REQUIRED",
            protocol::schema::ERROR,
            &format!("{} requires HELLO negotiation with capability 'tool_registry_v1'.", opcode),
        ));
    }

    None
}