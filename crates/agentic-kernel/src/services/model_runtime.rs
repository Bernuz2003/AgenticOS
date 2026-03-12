use std::path::PathBuf;

use crate::backend::{BackendCapabilities, BackendClass};
use crate::engine::LLMEngine;
use crate::model_catalog::{ModelCatalog, ResolvedModelTarget};
use crate::prompting::PromptFamily;

pub struct LoadedModelSummary {
    pub family: PromptFamily,
    pub backend_id: String,
    pub backend_class: BackendClass,
    pub backend_capabilities: BackendCapabilities,
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
        backend_class: target.driver_resolution.backend_class,
        backend_capabilities: target.driver_resolution.capabilities,
        driver_source: new_engine.driver_resolution_source().to_string(),
        driver_rationale: new_engine.driver_resolution_rationale().to_string(),
        path: target.path.clone(),
        architecture: target
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.architecture.clone()),
        load_mode: match target.driver_resolution.backend_class {
            BackendClass::ResidentLocal => "resident_local_adapter".to_string(),
            BackendClass::RemoteStateless => "remote_stateless".to_string(),
        },
    };

    *engine_state = Some(new_engine);

    if let Some(model_id) = target.model_id.as_ref() {
        let _ = model_catalog.set_selected(model_id);
    }

    Ok(summary)
}
