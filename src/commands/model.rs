use crate::engine::LLMEngine;
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
        Ok((resolved_path, family)) => {
            let tokenizer_hint = ctx
                .model_catalog
                .entries
                .iter()
                .find(|m| m.path == resolved_path)
                .and_then(|m| m.tokenizer_path.clone());

            // Drop-before-load: free old engine weights BEFORE allocating new.
            *ctx.engine_state = None;
            match LLMEngine::load(
                resolved_path.to_string_lossy().as_ref(),
                family,
                tokenizer_hint,
            ) {
                Ok(new_engine) => {
                    *ctx.engine_state = Some(new_engine);
                    *ctx.active_family = family;

                    if let Some(entry) = ctx
                        .model_catalog
                        .entries
                        .iter()
                        .find(|m| m.path == resolved_path)
                    {
                        ctx.model_catalog.selected_id = Some(entry.id.clone());
                    }

                    protocol::response_ok(&format!(
                        "Master Model Loaded. family={:?} path={}",
                        family,
                        resolved_path.display()
                    ))
                }
                Err(e) => protocol::response_err_code("LOAD_FAILED", &format!("{}", e)),
            }
        }
        Err(e) => protocol::response_err_code("MODEL_SELECTOR", &e.to_string()),
    }
}

pub(crate) fn handle_list_models(ctx: &mut CommandContext<'_>) -> Vec<u8> {
    let _ = ctx.model_catalog.refresh();
    protocol::response_ok(&ctx.model_catalog.format_list())
}

pub(crate) fn handle_select_model(ctx: &mut CommandContext<'_>, payload: &[u8]) -> Vec<u8> {
    let _ = ctx.model_catalog.refresh();
    let model_id = String::from_utf8_lossy(payload).trim().to_string();
    if model_id.is_empty() {
        protocol::response_err_code("MISSING_MODEL_ID", "SELECT_MODEL requires a model id")
    } else {
        match ctx.model_catalog.set_selected(&model_id) {
            Ok(_) => {
                if let Some(entry) = ctx.model_catalog.find_by_id(&model_id) {
                    *ctx.active_family = entry.family;
                }
                protocol::response_ok(&format!("Selected model '{}'.", model_id))
            }
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
        match ctx.model_catalog.format_info(&model_id) {
            Ok(info) => protocol::response_ok(&info),
            Err(e) => protocol::response_err_code("MODEL_INFO", &e.to_string()),
        }
    }
}
