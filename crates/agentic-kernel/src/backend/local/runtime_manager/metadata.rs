use crate::model_catalog::{infer_metadata_path, load_model_metadata, LocalLoadTarget, ModelMetadata};
use crate::prompting::PromptFamily;

use super::manager::RequestedLocalRuntime;
use super::paths::normalize_model_path;

impl RequestedLocalRuntime {
    pub(super) fn from_target(target: &LocalLoadTarget) -> Result<Self, String> {
        let model_path = normalize_model_path(&target.display_path);
        Ok(Self {
            family: target.family,
            model_path,
            logical_model_id: target
                .model_id
                .clone()
                .unwrap_or_else(|| target.display_path.display().to_string()),
            context_window_tokens: target
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.max_context_tokens),
        })
    }

    pub(super) fn from_reference(reference: &str, family: PromptFamily) -> Result<Self, String> {
        let fallback_path = std::path::PathBuf::from(reference);
        let model_path = normalize_model_path(&fallback_path);
        let metadata = load_local_runtime_metadata(&model_path);
        Ok(Self {
            family,
            model_path: model_path.clone(),
            logical_model_id: reference.to_string(),
            context_window_tokens: metadata.and_then(|entry| entry.max_context_tokens),
        })
    }
}

pub(super) fn load_local_runtime_metadata(model_path: &std::path::Path) -> Option<ModelMetadata> {
    infer_metadata_path(model_path)
        .as_ref()
        .and_then(|path| load_model_metadata(path))
}
