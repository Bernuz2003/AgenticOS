use crate::backend;
use crate::errors::CatalogError;
use crate::protocol;
use crate::services::model_runtime::activate_model_target;
use agentic_control_models::{KernelEvent, LoadModelResult, SelectModelResult};
use agentic_protocol::ControlErrorCode;
use serde_json::Value;

use super::context::ModelCommandContext;

pub(crate) fn handle_load(ctx: ModelCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    // Cannot load a new model while processes are in-flight on the inference worker.
    if !ctx.in_flight.is_empty() {
        return protocol::response_protocol_err_typed(
            ctx.client,
            &ctx.request_id,
            ControlErrorCode::InFlight,
            protocol::schema::ERROR,
            &format!(
                "Cannot LOAD while {} process(es) are in-flight. KILL them first.",
                ctx.in_flight.len()
            ),
        );
    }

    if let Some(engine) = ctx.engine_state.as_ref() {
        if !engine.processes.is_empty() {
            return protocol::response_protocol_err_typed(
                ctx.client,
                &ctx.request_id,
                ControlErrorCode::LoadBusy,
                protocol::schema::ERROR,
                &format!(
                    "Cannot LOAD while {} live process(es) are still present. TERM/KILL them first.",
                    engine.processes.len()
                ),
            );
        }
    }

    let _ = ctx.model_catalog.refresh();
    let selector = String::from_utf8_lossy(payload).trim().to_string();
    match ctx.model_catalog.resolve_load_target(&selector) {
        Ok(target) => match activate_model_target(ctx.engine_state, ctx.model_catalog, &target) {
            Ok(loaded) => {
                ctx.pending_events.push(KernelEvent::ModelChanged {
                    selected_model_id: ctx.model_catalog.selected_id.clone().unwrap_or_default(),
                    loaded_model_id: current_loaded_model_id(
                        ctx.model_catalog,
                        loaded.path.as_path(),
                    ),
                });
                ctx.pending_events.push(KernelEvent::LobbyChanged {
                    reason: "model_loaded".to_string(),
                });
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
                    &LoadModelResult {
                        family: format!("{:?}", loaded.family),
                        backend: loaded.backend_id,
                        driver_source: loaded.driver_source,
                        driver_rationale: loaded.driver_rationale,
                        path: loaded.path.display().to_string(),
                        architecture: loaded.architecture,
                        load_mode: loaded.load_mode,
                    },
                    Some(&message),
                )
            }
            Err(e) => protocol::response_protocol_err_typed(
                ctx.client,
                &ctx.request_id,
                ControlErrorCode::LoadFailed,
                protocol::schema::ERROR,
                &e.to_string(),
            ),
        },
        Err(CatalogError::DriverResolutionFailed(detail)) => protocol::response_protocol_err_typed(
            ctx.client,
            &ctx.request_id,
            ControlErrorCode::DriverUnresolved,
            protocol::schema::ERROR,
            &detail,
        ),
        Err(e) => protocol::response_protocol_err_typed(
            ctx.client,
            &ctx.request_id,
            ControlErrorCode::ModelSelector,
            protocol::schema::ERROR,
            &e.to_string(),
        ),
    }
}

pub(crate) fn handle_list_models(ctx: ModelCommandContext<'_>) -> Vec<u8> {
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

pub(crate) fn handle_select_model(ctx: ModelCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let _ = ctx.model_catalog.refresh();
    let model_id = String::from_utf8_lossy(payload).trim().to_string();
    if model_id.is_empty() {
        protocol::response_protocol_err_typed(
            ctx.client,
            &ctx.request_id,
            ControlErrorCode::MissingModelId,
            protocol::schema::ERROR,
            "SELECT_MODEL requires a model id",
        )
    } else {
        match ctx.model_catalog.set_selected(&model_id) {
            Ok(_) => {
                ctx.pending_events.push(KernelEvent::ModelChanged {
                    selected_model_id: ctx.model_catalog.selected_id.clone().unwrap_or_default(),
                    loaded_model_id: current_engine_loaded_model_id(
                        ctx.engine_state.as_ref(),
                        ctx.model_catalog,
                    ),
                });
                ctx.pending_events.push(KernelEvent::LobbyChanged {
                    reason: "model_selected".to_string(),
                });
                let message = format!("Selected model '{}'.", model_id);
                protocol::response_protocol_ok(
                    ctx.client,
                    &ctx.request_id,
                    "SELECT_MODEL",
                    protocol::schema::SELECT_MODEL,
                    &SelectModelResult {
                        selected_model: model_id,
                    },
                    Some(&message),
                )
            }
            Err(e) => protocol::response_protocol_err_typed(
                ctx.client,
                &ctx.request_id,
                ControlErrorCode::ModelNotFound,
                protocol::schema::ERROR,
                &e.to_string(),
            ),
        }
    }
}

fn current_loaded_model_id(
    model_catalog: &crate::model_catalog::ModelCatalog,
    loaded_path: &std::path::Path,
) -> String {
    let loaded_path = loaded_path.to_string_lossy();
    model_catalog
        .entries
        .iter()
        .find(|entry| entry.path.to_string_lossy() == loaded_path)
        .map(|entry| entry.id.clone())
        .unwrap_or_default()
}

fn current_engine_loaded_model_id(
    engine: Option<&crate::engine::LLMEngine>,
    model_catalog: &crate::model_catalog::ModelCatalog,
) -> String {
    let Some(engine) = engine else {
        return String::new();
    };

    current_loaded_model_id(
        model_catalog,
        std::path::Path::new(&engine.loaded_model_path()),
    )
}

pub(crate) fn handle_model_info(ctx: ModelCommandContext<'_>, payload: &[u8]) -> Vec<u8> {
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
        protocol::response_protocol_err_typed(
            ctx.client,
            &ctx.request_id,
            ControlErrorCode::Generic,
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
            Err(e) => protocol::response_protocol_err_typed(
                ctx.client,
                &ctx.request_id,
                ControlErrorCode::Generic,
                protocol::schema::ERROR,
                &e.to_string(),
            ),
        }
    }
}

pub(crate) fn handle_backend_diag(ctx: ModelCommandContext<'_>) -> Vec<u8> {
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
        Err(err) => protocol::response_protocol_err_typed(
            ctx.client,
            &ctx.request_id,
            ControlErrorCode::BackendDiag,
            protocol::schema::ERROR,
            &err.to_string(),
        ),
    }
}
