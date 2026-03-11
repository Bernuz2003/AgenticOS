use agentic_control_models::{
    ModelCatalogEntry, ModelCatalogSnapshot, ModelInfoResponse, ModelRoutingRecommendation,
};

use crate::errors::CatalogError;

use super::driver::driver_view_for_entry;
use super::routing::recommend_for_workload;
use super::{ModelCatalog, ModelEntry, WorkloadClass};

pub(super) fn format_list_json(catalog: &ModelCatalog) -> String {
    let selected = catalog.selected_id.as_deref();
    let payload = ModelCatalogSnapshot {
        selected_model_id: catalog.selected_id.clone(),
        total_models: catalog.entries.len(),
        models: catalog
            .entries
            .iter()
            .map(|entry| format_list_entry(entry, selected))
            .collect(),
        routing_recommendations: [
            ("fast", WorkloadClass::Fast),
            ("general", WorkloadClass::General),
            ("code", WorkloadClass::Code),
            ("reasoning", WorkloadClass::Reasoning),
        ]
        .into_iter()
        .map(|(workload, class)| {
            let decision = recommend_for_workload(&catalog.entries, class);
            let picked = decision.entry;
            ModelRoutingRecommendation {
                workload: workload.to_string(),
                model_id: picked.map(|m| m.id.clone()),
                family: picked.map(|m| format!("{:?}", m.family)),
                backend_preference: picked
                    .and_then(|m| m.metadata.as_ref())
                    .and_then(|meta| meta.backend_preference.clone()),
                resolved_backend: picked
                    .map(driver_view_for_entry)
                    .and_then(|driver| driver.resolved_backend),
                driver_resolution_source: picked
                    .map(driver_view_for_entry)
                    .map(|driver| driver.driver_resolution_source)
                    .unwrap_or_else(|| "unresolved".to_string()),
                driver_resolution_rationale: picked
                    .map(driver_view_for_entry)
                    .map(|driver| driver.driver_resolution_rationale)
                    .unwrap_or_else(|| "no model selected for this workload".to_string()),
                driver_available: picked
                    .map(driver_view_for_entry)
                    .and_then(|driver| driver.driver_available),
                driver_load_supported: picked
                    .map(driver_view_for_entry)
                    .and_then(|driver| driver.driver_load_supported),
                metadata_source: picked.and_then(|m| m.metadata_source.clone()),
                source: decision.source.to_string(),
                rationale: decision.rationale,
                capability_key: decision.capability_key.map(str::to_string),
                capability_score: decision.capability_score,
            }
        })
        .collect(),
    };

    serde_json::to_string(&payload).expect("ModelCatalogSnapshot is serializable")
}

pub(super) fn format_info_json(
    catalog: &ModelCatalog,
    model_id: &str,
) -> Result<String, CatalogError> {
    let entry = catalog
        .find_by_id(model_id)
        .ok_or_else(|| CatalogError::ModelNotFound(model_id.to_string()))?;
    let driver = driver_view_for_entry(entry);

    let payload = ModelInfoResponse {
        id: entry.id.clone(),
        family: format!("{:?}", entry.family),
        architecture: entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.architecture.clone()),
        path: entry.path.display().to_string(),
        tokenizer_path: entry
            .tokenizer_path
            .as_ref()
            .map(|p| p.display().to_string()),
        tokenizer_present: entry.tokenizer_path.is_some(),
        metadata_source: entry.metadata_source.clone(),
        backend_preference: entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.backend_preference.clone()),
        resolved_backend: driver.resolved_backend,
        driver_resolution_source: driver.driver_resolution_source,
        driver_resolution_rationale: driver.driver_resolution_rationale,
        driver_available: driver.driver_available,
        driver_load_supported: driver.driver_load_supported,
        chat_template: entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.chat_template.clone()),
        assistant_preamble: entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.assistant_preamble.clone()),
        special_tokens: entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.special_tokens.clone()),
        stop_markers: entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.stop_markers.clone()),
        capabilities: entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.capabilities.clone()),
        selected: catalog.selected_id.as_deref() == Some(entry.id.as_str()),
    };

    Ok(serde_json::to_string(&payload).expect("ModelInfoResponse is serializable"))
}

fn format_list_entry(entry: &ModelEntry, selected: Option<&str>) -> ModelCatalogEntry {
    let driver = driver_view_for_entry(entry);
    ModelCatalogEntry {
        id: entry.id.clone(),
        family: format!("{:?}", entry.family),
        architecture: entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.architecture.clone()),
        path: entry.path.display().to_string(),
        tokenizer_path: entry
            .tokenizer_path
            .as_ref()
            .map(|p| p.display().to_string()),
        tokenizer_present: entry.tokenizer_path.is_some(),
        metadata_source: entry.metadata_source.clone(),
        backend_preference: entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.backend_preference.clone()),
        resolved_backend: driver.resolved_backend,
        driver_resolution_source: driver.driver_resolution_source,
        driver_resolution_rationale: driver.driver_resolution_rationale,
        driver_available: driver.driver_available,
        driver_load_supported: driver.driver_load_supported,
        capabilities: entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.capabilities.clone()),
        selected: selected == Some(entry.id.as_str()),
    }
}
