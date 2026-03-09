use crate::backend;
use crate::errors::CatalogError;
use crate::protocol;
use crate::services::model_runtime::activate_model_target;
use serde_json::Value;

use super::context::CommandContext;

pub(crate) fn handle_load(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    // Cannot load a new model while processes are in-flight on the inference worker.
    if !ctx.in_flight.is_empty() {
        return protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "IN_FLIGHT",
            protocol::schema::ERROR,
            &format!(
                "Cannot LOAD while {} process(es) are in-flight. KILL them first.",
                ctx.in_flight.len()
            ),
        );
    }

    let _ = ctx.model_catalog.refresh();
    let selector = String::from_utf8_lossy(payload).trim().to_string();
    match ctx.model_catalog.resolve_load_target(&selector) {
        Ok(target) => {
            match activate_model_target(ctx.engine_state, ctx.model_catalog, &target) {
                Ok(loaded) => {
                    let message = format!(
                        "Master Model Loaded. family={:?} backend={} driver_source={} rationale={} path={}",
                        loaded.family,
                        loaded.backend_id,
                        loaded.driver_source,
                        loaded.driver_rationale,
                        loaded.path.display()
                    );
                    protocol::response_protocol_ok(
                        ctx.client,
                        &ctx.request_id,
                        "LOAD",
                        protocol::schema::LOAD,
                        &serde_json::json!({
                            "family": format!("{:?}", loaded.family),
                            "backend": loaded.backend_id,
                            "driver_source": loaded.driver_source,
                            "driver_rationale": loaded.driver_rationale,
                            "path": loaded.path.display().to_string(),
                        }),
                        Some(&message),
                    )
                }
                Err(e) => protocol::response_protocol_err(
                    ctx.client,
                    &ctx.request_id,
                    "LOAD_FAILED",
                    protocol::schema::ERROR,
                    &e.to_string(),
                ),
            }
        }
        Err(CatalogError::DriverResolutionFailed(detail)) => {
            protocol::response_protocol_err(
                ctx.client,
                &ctx.request_id,
                "DRIVER_UNRESOLVED",
                protocol::schema::ERROR,
                &detail,
            )
        }
        Err(e) => protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "MODEL_SELECTOR",
            protocol::schema::ERROR,
            &e.to_string(),
        ),
    }
}

pub(crate) fn handle_list_models(ctx: &mut CommandContext<'_>) -> Vec<u8> {
    let _ = ctx.model_catalog.refresh();
    let payload = ctx.model_catalog.format_list_json();
    let data: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
    protocol::response_protocol_ok(
        ctx.client,
        &ctx.request_id,
        "LIST_MODELS",
        protocol::schema::LIST_MODELS,
        &data,
        Some(&payload),
    )
}

pub(crate) fn handle_select_model(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let _ = ctx.model_catalog.refresh();
    let model_id = String::from_utf8_lossy(payload).trim().to_string();
    if model_id.is_empty() {
        protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "MISSING_MODEL_ID",
            protocol::schema::ERROR,
            "SELECT_MODEL requires a model id",
        )
    } else {
        match ctx.model_catalog.set_selected(&model_id) {
            Ok(_) => {
                let message = format!("Selected model '{}'.", model_id);
                protocol::response_protocol_ok(
                    ctx.client,
                    &ctx.request_id,
                    "SELECT_MODEL",
                    protocol::schema::SELECT_MODEL,
                    &serde_json::json!({"selected_model": model_id}),
                    Some(&message),
                )
            }
            Err(e) => protocol::response_protocol_err(
                ctx.client,
                &ctx.request_id,
                "MODEL_NOT_FOUND",
                protocol::schema::ERROR,
                &e.to_string(),
            ),
        }
    }
}

pub(crate) fn handle_model_info(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let _ = ctx.model_catalog.refresh();
    let requested = String::from_utf8_lossy(payload).trim().to_string();
    let model_id = if requested.is_empty() {
        if let Some(selected) = &ctx.model_catalog.selected_id {
            selected.clone()
        } else {
            String::new()
        }
    } else {
        requested
    };

    if model_id.is_empty() {
        protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "GENERIC",
            protocol::schema::ERROR,
            "MODEL_INFO requires a model id or an active selected model",
        )
    } else {
        match ctx.model_catalog.format_info_json(&model_id) {
            Ok(info) => {
                let data: Value = serde_json::from_str(&info).unwrap_or(Value::Null);
                protocol::response_protocol_ok(
                    ctx.client,
                    &ctx.request_id,
                    "MODEL_INFO",
                    protocol::schema::MODEL_INFO,
                    &data,
                    Some(&info),
                )
            }
            Err(e) => protocol::response_protocol_err(
                ctx.client,
                &ctx.request_id,
                "MODEL_INFO",
                protocol::schema::ERROR,
                &e.to_string(),
            ),
        }
    }
}

pub(crate) fn handle_backend_diag(ctx: &mut CommandContext<'_>) -> Vec<u8> {
    match backend::diagnose_external_backend() {
        Ok(report) => {
            let payload = report.to_string();
            let data: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
            protocol::response_protocol_ok(
                ctx.client,
                &ctx.request_id,
                "BACKEND_DIAG",
                protocol::schema::BACKEND_DIAG,
                &data,
                Some(&payload),
            )
        }
        Err(err) => protocol::response_protocol_err(
            ctx.client,
            &ctx.request_id,
            "BACKEND_DIAG",
            protocol::schema::ERROR,
            &err.to_string(),
        ),
    }
}
