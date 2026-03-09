use crate::backend;
use crate::errors::CatalogError;
use crate::protocol;
use crate::services::model_runtime::activate_model_target;

use super::context::CommandContext;

pub(crate) fn handle_load(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    // Cannot load a new model while processes are in-flight on the inference worker.
    if !ctx.in_flight.is_empty() {
        return protocol::response_err_code(
            "IN_FLIGHT",
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
                    protocol::response_ok(&format!(
                        "Master Model Loaded. family={:?} backend={} driver_source={} rationale={} path={}",
                        loaded.family,
                        loaded.backend_id,
                        loaded.driver_source,
                        loaded.driver_rationale,
                        loaded.path.display()
                    ))
                }
                Err(e) => protocol::response_err_code("LOAD_FAILED", &format!("{}", e)),
            }
        }
        Err(CatalogError::DriverResolutionFailed(detail)) => {
            protocol::response_err_code("DRIVER_UNRESOLVED", &detail)
        }
        Err(e) => protocol::response_err_code("MODEL_SELECTOR", &e.to_string()),
    }
}

pub(crate) fn handle_list_models(ctx: &mut CommandContext<'_>) -> Vec<u8> {
    let _ = ctx.model_catalog.refresh();
    protocol::response_ok_code("LIST_MODELS", &ctx.model_catalog.format_list_json())
}

pub(crate) fn handle_select_model(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let _ = ctx.model_catalog.refresh();
    let model_id = String::from_utf8_lossy(payload).trim().to_string();
    if model_id.is_empty() {
        protocol::response_err_code("MISSING_MODEL_ID", "SELECT_MODEL requires a model id")
    } else {
        match ctx.model_catalog.set_selected(&model_id) {
            Ok(_) => protocol::response_ok(&format!("Selected model '{}'.", model_id)),
            Err(e) => protocol::response_err_code("MODEL_NOT_FOUND", &e.to_string()),
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
        protocol::response_err("MODEL_INFO requires a model id or an active selected model")
    } else {
        match ctx.model_catalog.format_info_json(&model_id) {
            Ok(info) => protocol::response_ok_code("MODEL_INFO", &info),
            Err(e) => protocol::response_err_code("MODEL_INFO", &e.to_string()),
        }
    }
}

pub(crate) fn handle_backend_diag(_ctx: &mut CommandContext<'_>) -> Vec<u8> {
    match backend::diagnose_external_backend() {
        Ok(report) => protocol::response_ok_code("BACKEND_DIAG", &report.to_string()),
        Err(err) => protocol::response_err_code("BACKEND_DIAG", &err.to_string()),
    }
}
