use std::path::PathBuf;

use agentic_control_models::RemoteModelRuntimeView;

use crate::backend::{BackendCapabilities, BackendClass};
use crate::engine::LLMEngine;
use crate::model_catalog::{ModelCatalog, ResolvedModelTarget};
use crate::prompting::PromptFamily;

pub struct LoadedModelSummary {
    pub family: PromptFamily,
    pub loaded_model_id: String,
    pub loaded_target_kind: String,
    pub loaded_provider_id: Option<String>,
    pub loaded_remote_model_id: Option<String>,
    pub backend_id: String,
    pub backend_class: BackendClass,
    pub backend_capabilities: BackendCapabilities,
    pub driver_source: String,
    pub driver_rationale: String,
    pub path: PathBuf,
    pub architecture: Option<String>,
    pub load_mode: String,
    pub remote_model: Option<RemoteModelRuntimeView>,
}

pub fn activate_model_target(
    engine_state: &mut Option<LLMEngine>,
    model_catalog: &mut ModelCatalog,
    target: &ResolvedModelTarget,
) -> Result<LoadedModelSummary, String> {
    let new_engine = LLMEngine::load_target(target).map_err(|e| e.to_string())?;
    let summary = LoadedModelSummary {
        family: new_engine.loaded_family(),
        loaded_model_id: target.logical_model_id(),
        loaded_target_kind: target.target_kind().to_string(),
        loaded_provider_id: target.provider_id().map(ToString::to_string),
        loaded_remote_model_id: target.remote_model_id().map(ToString::to_string),
        backend_id: new_engine.loaded_backend_id().to_string(),
        backend_class: new_engine.loaded_backend_class(),
        backend_capabilities: new_engine.loaded_backend_capabilities(),
        driver_source: new_engine.driver_resolution_source().to_string(),
        driver_rationale: new_engine.driver_resolution_rationale().to_string(),
        path: target.display_path().to_path_buf(),
        architecture: target.architecture(),
        load_mode: match new_engine.loaded_backend_class() {
            BackendClass::ResidentLocal => "resident_local_adapter".to_string(),
            BackendClass::RemoteStateless => "remote_stateless".to_string(),
        },
        remote_model: target.remote_model_view(),
    };

    *engine_state = Some(new_engine);

    if let Some(model_id) = target.local_model_id() {
        let _ = model_catalog.set_selected(model_id);
    }

    Ok(summary)
}
