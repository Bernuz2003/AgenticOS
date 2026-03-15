use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::config::RemoteAdapterKind;
use crate::errors::CatalogError;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct RemoteProviderEntry {
    pub id: String,
    pub backend_id: String,
    pub adapter_kind: RemoteAdapterKind,
    pub label: String,
    pub note: Option<String>,
    pub credential_hint: Option<String>,
    pub default_model_id: String,
    pub models: Vec<RemoteModelEntry>,
}

#[derive(Debug, Clone)]
pub struct RemoteModelEntry {
    pub id: String,
    pub label: String,
    pub context_window_tokens: Option<usize>,
    pub max_output_tokens: Option<usize>,
    pub supports_structured_output: bool,
    pub input_price_usd_per_mtok: Option<f64>,
    pub output_price_usd_per_mtok: Option<f64>,
}

pub(super) struct LoadedRemoteProviderCatalog {
    pub(super) providers: Vec<RemoteProviderEntry>,
    pub(super) fingerprint: u64,
}

#[derive(Debug, Default, Deserialize)]
struct RemoteProviderCatalogFile {
    #[serde(default)]
    providers: Vec<RemoteProviderFileEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct RemoteProviderFileEntry {
    id: String,
    #[serde(default)]
    backend_id: Option<String>,
    #[serde(default)]
    adapter: RemoteAdapterKind,
    label: String,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    credential_hint: Option<String>,
    default_model_id: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    models: Vec<RemoteModelFileEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct RemoteModelFileEntry {
    id: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    context_window_tokens: Option<usize>,
    #[serde(default)]
    max_output_tokens: Option<usize>,
    #[serde(default)]
    supports_structured_output: bool,
    #[serde(default)]
    input_price_usd_per_mtok: Option<f64>,
    #[serde(default)]
    output_price_usd_per_mtok: Option<f64>,
}

pub(super) fn load_remote_provider_catalog() -> Result<LoadedRemoteProviderCatalog, CatalogError> {
    let path = crate::config::kernel_config()
        .paths
        .remote_provider_catalog_path
        .clone();
    load_remote_provider_catalog_from_path(&path)
}

fn load_remote_provider_catalog_from_path(
    path: &Path,
) -> Result<LoadedRemoteProviderCatalog, CatalogError> {
    if !path.exists() {
        return Ok(LoadedRemoteProviderCatalog {
            providers: Vec::new(),
            fingerprint: 0,
        });
    }

    let raw = fs::read_to_string(path).map_err(|err| CatalogError::RemoteProviderCatalogRead {
        path: path.display().to_string(),
        detail: err.to_string(),
    })?;
    let parsed = toml::from_str::<RemoteProviderCatalogFile>(&raw).map_err(|err| {
        CatalogError::RemoteProviderCatalogInvalid {
            path: path.display().to_string(),
            detail: err.to_string(),
        }
    })?;

    let mut providers = parsed
        .providers
        .into_iter()
        .filter(|provider| provider.enabled)
        .map(|provider| {
            let models = provider
                .models
                .into_iter()
                .map(|model| RemoteModelEntry {
                    label: model.label.unwrap_or_else(|| model.id.clone()),
                    id: model.id,
                    context_window_tokens: model.context_window_tokens,
                    max_output_tokens: model.max_output_tokens,
                    supports_structured_output: model.supports_structured_output,
                    input_price_usd_per_mtok: model.input_price_usd_per_mtok,
                    output_price_usd_per_mtok: model.output_price_usd_per_mtok,
                })
                .collect::<Vec<_>>();

            RemoteProviderEntry {
                id: provider.id.clone(),
                backend_id: provider.backend_id.unwrap_or_else(|| provider.id.clone()),
                adapter_kind: provider.adapter,
                label: provider.label,
                note: provider.note,
                credential_hint: provider.credential_hint,
                default_model_id: provider.default_model_id,
                models,
            }
        })
        .collect::<Vec<_>>();
    providers.sort_by(|left, right| left.id.cmp(&right.id));

    Ok(LoadedRemoteProviderCatalog {
        providers,
        fingerprint: compute_file_fingerprint(path)?,
    })
}

fn compute_file_fingerprint(path: &Path) -> Result<u64, CatalogError> {
    let metadata = fs::metadata(path).map_err(|err| CatalogError::RemoteProviderCatalogRead {
        path: path.display().to_string(),
        detail: err.to_string(),
    })?;

    let mut hasher = DefaultHasher::new();
    path.display().to_string().hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    match metadata.modified() {
        Ok(modified) => {
            if let Ok(since_epoch) = modified.duration_since(std::time::UNIX_EPOCH) {
                since_epoch.as_secs().hash(&mut hasher);
                since_epoch.subsec_nanos().hash(&mut hasher);
            }
        }
        Err(_) => 0_u8.hash(&mut hasher),
    }
    Ok(hasher.finish())
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
#[path = "remote_catalog_tests.rs"]
mod tests;
