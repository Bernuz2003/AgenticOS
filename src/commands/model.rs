use crate::engine::LLMEngine;
use crate::errors::CatalogError;
use crate::protocol;

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
            // Drop-before-load: free old engine weights BEFORE allocating new.
            *ctx.engine_state = None;
            match LLMEngine::load_target(&target) {
                Ok(new_engine) => {
                    let loaded_family = new_engine.loaded_family();
                    let loaded_backend = new_engine.loaded_backend_id().to_string();
                    let driver_source = new_engine.driver_resolution_source().to_string();
                    let driver_rationale = new_engine.driver_resolution_rationale().to_string();
                    *ctx.engine_state = Some(new_engine);

                    if let Some(model_id) = target.model_id.as_ref() {
                        ctx.model_catalog.selected_id = Some(model_id.clone());
                    }

                    protocol::response_ok(&format!(
                        "Master Model Loaded. family={:?} backend={} driver_source={} rationale={} path={}",
                        loaded_family,
                        loaded_backend,
                        driver_source,
                        driver_rationale,
                        target.path.display()
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
