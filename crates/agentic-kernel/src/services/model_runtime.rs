use std::path::PathBuf;

use crate::engine::LLMEngine;
use crate::model_catalog::{ModelCatalog, ResolvedModelTarget};
use crate::prompting::PromptFamily;

pub struct LoadedModelSummary {
    pub family: PromptFamily,
    pub backend_id: String,
    pub driver_source: String,
    pub driver_rationale: String,
    pub path: PathBuf,
    pub architecture: Option<String>,
    pub load_mode: String,
}

pub fn activate_model_target(
    engine_state: &mut Option<LLMEngine>,
    model_catalog: &mut ModelCatalog,
    target: &ResolvedModelTarget,
) -> Result<LoadedModelSummary, String> {
    let new_engine = LLMEngine::load_target(target).map_err(|e| e.to_string())?;
    let summary = LoadedModelSummary {
        family: new_engine.loaded_family(),
        backend_id: new_engine.loaded_backend_id().to_string(),
        driver_source: new_engine.driver_resolution_source().to_string(),
        driver_rationale: new_engine.driver_resolution_rationale().to_string(),
        path: target.path.clone(),
        architecture: target
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.architecture.clone()),
        load_mode: if target.driver_resolution.resolved_backend_id == "external-llamacpp" {
            "remote_adapter".to_string()
        } else {
            "in_process".to_string()
        },
    };

    *engine_state = Some(new_engine);

    if let Some(model_id) = target.model_id.as_ref() {
        let _ = model_catalog.set_selected(model_id);
    }

    Ok(summary)
}
