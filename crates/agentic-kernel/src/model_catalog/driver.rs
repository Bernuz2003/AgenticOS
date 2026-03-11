use crate::backend::{resolve_driver_for_model, DriverResolution};
use crate::errors::CatalogError;

use super::ModelEntry;

pub(super) struct DriverCatalogView {
    pub(super) resolved_backend: Option<String>,
    pub(super) driver_resolution_source: String,
    pub(super) driver_resolution_rationale: String,
    pub(super) driver_available: Option<bool>,
    pub(super) driver_load_supported: Option<bool>,
}

pub(super) fn driver_view_for_entry(entry: &ModelEntry) -> DriverCatalogView {
    match resolve_driver_for_entry(entry) {
        Ok(resolution) => DriverCatalogView {
            resolved_backend: Some(resolution.resolved_backend_id),
            driver_resolution_source: resolution.resolution_source.to_string(),
            driver_resolution_rationale: resolution.resolution_rationale,
            driver_available: Some(resolution.available),
            driver_load_supported: Some(resolution.load_supported),
        },
        Err(err) => DriverCatalogView {
            resolved_backend: None,
            driver_resolution_source: "unresolved".to_string(),
            driver_resolution_rationale: err.to_string(),
            driver_available: None,
            driver_load_supported: None,
        },
    }
}

pub(crate) fn resolve_driver_for_entry(
    entry: &ModelEntry,
) -> Result<DriverResolution, CatalogError> {
    resolve_driver_for_model(
        entry.family,
        entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.architecture.as_deref()),
        entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.backend_preference.as_deref()),
    )
    .map_err(CatalogError::DriverResolutionFailed)
}
