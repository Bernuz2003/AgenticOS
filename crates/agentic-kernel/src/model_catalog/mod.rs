use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;

use crate::errors::CatalogError;
use crate::prompting::PromptFamily;

mod cache;
mod discovery;
mod driver;
mod formatting;
mod metadata;
mod remote_catalog;
mod routing;
#[cfg(test)]
mod tests;
mod workload;

pub use metadata::ModelMetadata;
pub use remote_catalog::{RemoteModelEntry, RemoteProviderEntry};
pub use workload::{infer_workload_class, parse_workload_label, WorkloadClass};

use cache::RenderCache;
use discovery::{build_entry, compute_catalog_fingerprint, discover_entries};
use driver::resolve_driver_for_entry;
use formatting::{format_info_json, format_list_json};
use remote_catalog::load_remote_provider_catalog;
use routing::select_for_workload;

#[derive(Debug, Clone)]
pub struct ModelEntry {
    pub id: String,
    pub path: PathBuf,
    pub family: PromptFamily,
    pub tokenizer_path: Option<PathBuf>,
    pub metadata_source: Option<String>,
    pub metadata: Option<ModelMetadata>,
}

#[derive(Debug, Clone)]
pub struct LocalLoadTarget {
    pub model_id: Option<String>,
    pub display_path: PathBuf,
    pub runtime_reference: String,
    pub family: PromptFamily,
    pub tokenizer_path: Option<PathBuf>,
    pub metadata: Option<ModelMetadata>,
    pub driver_resolution: crate::backend::DriverResolution,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RemoteLoadTarget {
    pub provider_id: String,
    pub provider_label: String,
    pub backend_id: String,
    pub model_id: String,
    pub model_spec: RemoteModelEntry,
    pub runtime_config: crate::config::RemoteProviderRuntimeConfig,
    pub display_path: PathBuf,
    pub runtime_reference: String,
    pub family: PromptFamily,
    pub tokenizer_path: Option<PathBuf>,
    pub driver_resolution: crate::backend::DriverResolution,
}

#[derive(Debug, Clone)]
pub enum ResolvedModelTarget {
    Local(LocalLoadTarget),
    Remote(RemoteLoadTarget),
}

impl ResolvedModelTarget {
    pub fn local(
        model_id: Option<String>,
        path: PathBuf,
        family: PromptFamily,
        tokenizer_path: Option<PathBuf>,
        metadata: Option<ModelMetadata>,
        driver_resolution: crate::backend::DriverResolution,
    ) -> Self {
        Self::Local(LocalLoadTarget {
            model_id,
            runtime_reference: path.to_string_lossy().to_string(),
            display_path: path,
            family,
            tokenizer_path,
            metadata,
            driver_resolution,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn remote(
        provider_id: impl Into<String>,
        provider_label: impl Into<String>,
        backend_id: impl Into<String>,
        model_id: impl Into<String>,
        model_spec: RemoteModelEntry,
        runtime_config: crate::config::RemoteProviderRuntimeConfig,
        tokenizer_path: Option<PathBuf>,
        driver_resolution: crate::backend::DriverResolution,
    ) -> Self {
        let provider_id = provider_id.into();
        let provider_label = provider_label.into();
        let backend_id = backend_id.into();
        let model_id = model_id.into();
        Self::Remote(RemoteLoadTarget {
            display_path: PathBuf::from(format!("cloud/{provider_id}/{model_id}")),
            runtime_reference: model_id.clone(),
            provider_id,
            provider_label,
            backend_id,
            model_id,
            model_spec,
            runtime_config,
            family: PromptFamily::Unknown,
            tokenizer_path,
            driver_resolution,
        })
    }

    pub fn display_path(&self) -> &Path {
        match self {
            Self::Local(target) => &target.display_path,
            Self::Remote(target) => &target.display_path,
        }
    }

    pub fn runtime_reference(&self) -> &str {
        match self {
            Self::Local(target) => &target.runtime_reference,
            Self::Remote(target) => &target.runtime_reference,
        }
    }

    pub fn family(&self) -> PromptFamily {
        match self {
            Self::Local(target) => target.family,
            Self::Remote(target) => target.family,
        }
    }

    pub fn tokenizer_path(&self) -> Option<&PathBuf> {
        match self {
            Self::Local(target) => target.tokenizer_path.as_ref(),
            Self::Remote(target) => target.tokenizer_path.as_ref(),
        }
    }

    pub fn cloned_tokenizer_path(&self) -> Option<PathBuf> {
        self.tokenizer_path().cloned()
    }

    pub fn metadata(&self) -> Option<&ModelMetadata> {
        match self {
            Self::Local(target) => target.metadata.as_ref(),
            Self::Remote(_) => None,
        }
    }

    pub fn cloned_metadata(&self) -> Option<ModelMetadata> {
        self.metadata().cloned()
    }

    pub fn driver_resolution(&self) -> &crate::backend::DriverResolution {
        match self {
            Self::Local(target) => &target.driver_resolution,
            Self::Remote(target) => &target.driver_resolution,
        }
    }

    pub fn architecture(&self) -> Option<String> {
        self.metadata()
            .and_then(|metadata| metadata.architecture.clone())
    }

    pub fn local_model_id(&self) -> Option<&str> {
        match self {
            Self::Local(target) => target.model_id.as_deref(),
            Self::Remote(_) => None,
        }
    }

    pub fn remote_model_id(&self) -> Option<&str> {
        match self {
            Self::Local(_) => None,
            Self::Remote(target) => Some(target.model_id.as_str()),
        }
    }

    #[allow(dead_code)]
    pub fn provider_id(&self) -> Option<&str> {
        match self {
            Self::Local(_) => None,
            Self::Remote(target) => Some(target.provider_id.as_str()),
        }
    }

    pub fn remote_target(&self) -> Option<&RemoteLoadTarget> {
        match self {
            Self::Local(_) => None,
            Self::Remote(target) => Some(target),
        }
    }

    pub fn target_kind(&self) -> &'static str {
        match self {
            Self::Local(target) if target.model_id.is_some() => "local_catalog",
            Self::Local(_) => "local_path",
            Self::Remote(_) => "remote_provider",
        }
    }

    pub fn logical_model_id(&self) -> String {
        if let Some(model_id) = self.local_model_id() {
            return model_id.to_string();
        }
        if let Some(model_id) = self.remote_model_id() {
            return model_id.to_string();
        }
        self.display_path().display().to_string()
    }

    pub fn remote_model_view(&self) -> Option<agentic_control_models::RemoteModelRuntimeView> {
        self.remote_target()
            .map(|target| agentic_control_models::RemoteModelRuntimeView {
                provider_id: target.provider_id.clone(),
                provider_label: target.provider_label.clone(),
                backend_id: target.backend_id.clone(),
                adapter_kind: target.runtime_config.adapter_kind.as_str().to_string(),
                model_id: target.model_id.clone(),
                model_label: target.model_spec.label.clone(),
                context_window_tokens: target.model_spec.context_window_tokens,
                max_output_tokens: target.model_spec.max_output_tokens,
                supports_structured_output: target.model_spec.supports_structured_output,
                input_price_usd_per_mtok: target.model_spec.input_price_usd_per_mtok,
                output_price_usd_per_mtok: target.model_spec.output_price_usd_per_mtok,
            })
    }
}

#[derive(Debug)]
pub struct ModelCatalog {
    pub models_dir: PathBuf,
    pub entries: Vec<ModelEntry>,
    pub remote_providers: Vec<RemoteProviderEntry>,
    pub selected_id: Option<String>,
    catalog_fingerprint: u64,
    render_cache: RefCell<RenderCache>,
}

impl ModelCatalog {
    pub fn discover(models_dir: impl Into<PathBuf>) -> Result<Self, CatalogError> {
        let models_dir = models_dir.into();
        let local_fingerprint = compute_catalog_fingerprint(&models_dir)?;
        let remote_catalog = load_remote_provider_catalog()?;
        let catalog_fingerprint = local_fingerprint ^ remote_catalog.fingerprint.rotate_left(1);
        Self::discover_with_fingerprint(models_dir, catalog_fingerprint, remote_catalog.providers)
    }

    fn discover_with_fingerprint(
        models_dir: PathBuf,
        catalog_fingerprint: u64,
        remote_providers: Vec<RemoteProviderEntry>,
    ) -> Result<Self, CatalogError> {
        Ok(Self {
            entries: discover_entries(&models_dir)?,
            remote_providers,
            models_dir,
            selected_id: None,
            catalog_fingerprint,
            render_cache: RefCell::new(RenderCache::default()),
        })
    }

    pub fn refresh(&mut self) -> Result<(), CatalogError> {
        let local_fingerprint = compute_catalog_fingerprint(&self.models_dir)?;
        let remote_catalog = load_remote_provider_catalog()?;
        let latest_fingerprint = local_fingerprint ^ remote_catalog.fingerprint.rotate_left(1);
        if latest_fingerprint == self.catalog_fingerprint {
            return Ok(());
        }

        let selected = self.selected_id.clone();
        let mut newer = ModelCatalog::discover_with_fingerprint(
            self.models_dir.clone(),
            latest_fingerprint,
            remote_catalog.providers,
        )?;
        if let Some(sel) = selected {
            if newer.entries.iter().any(|entry| entry.id == sel) {
                newer.selected_id = Some(sel);
            }
        }
        *self = newer;
        Ok(())
    }

    pub fn set_selected(&mut self, model_id: &str) -> Result<(), CatalogError> {
        if self.entries.iter().any(|entry| entry.id == model_id) {
            self.selected_id = Some(model_id.to_string());
            self.invalidate_render_cache();
            Ok(())
        } else {
            Err(CatalogError::ModelNotFound(model_id.to_string()))
        }
    }

    pub fn clear_selected(&mut self) {
        self.selected_id = None;
        self.invalidate_render_cache();
    }

    pub fn selected_entry(&self) -> Option<&ModelEntry> {
        self.selected_id
            .as_ref()
            .and_then(|id| self.entries.iter().find(|entry| &entry.id == id))
    }

    pub fn find_by_id(&self, model_id: &str) -> Option<&ModelEntry> {
        self.entries.iter().find(|entry| entry.id == model_id)
    }

    pub fn remote_provider(&self, provider_id: &str) -> Option<&RemoteProviderEntry> {
        self.remote_providers
            .iter()
            .find(|provider| provider.id == provider_id)
    }

    pub fn resolve_load_target(&self, payload: &str) -> Result<ResolvedModelTarget, CatalogError> {
        let raw = payload.trim();

        if raw.is_empty() {
            if let Some(entry) = self.selected_entry() {
                return self.resolve_entry_target(entry);
            }
            return Err(CatalogError::NoModelSelected);
        }

        if raw.starts_with("cloud:") {
            return self.resolve_cloud_target(raw);
        }

        if let Some(entry) = self.find_by_id(raw) {
            return self.resolve_entry_target(entry);
        }

        if raw.ends_with(".gguf") || raw.contains('/') || raw.contains('\\') {
            let path = PathBuf::from(raw);
            if !path.exists() {
                return Err(CatalogError::PathNotFound(path.display().to_string()));
            }
            return self.resolve_path_target(path);
        }

        Err(CatalogError::InvalidSelector(raw.to_string()))
    }

    pub fn resolve_workload_target(
        &self,
        class: WorkloadClass,
    ) -> Result<Option<ResolvedModelTarget>, CatalogError> {
        self.select_for_workload(class)
            .map(|entry| self.resolve_entry_target(entry))
            .transpose()
    }

    pub fn resolve_entry_target(
        &self,
        entry: &ModelEntry,
    ) -> Result<ResolvedModelTarget, CatalogError> {
        Ok(ResolvedModelTarget::local(
            Some(entry.id.clone()),
            entry.path.clone(),
            entry.family,
            entry.tokenizer_path.clone(),
            entry.metadata.clone(),
            resolve_driver_for_entry(entry)?,
        ))
    }

    fn resolve_path_target(&self, path: PathBuf) -> Result<ResolvedModelTarget, CatalogError> {
        let entry = build_entry(&self.models_dir, path.clone());

        Ok(ResolvedModelTarget::local(
            None,
            path,
            entry.family,
            entry.tokenizer_path.clone(),
            entry.metadata.clone(),
            resolve_driver_for_entry(&entry)?,
        ))
    }

    fn resolve_cloud_target(&self, raw: &str) -> Result<ResolvedModelTarget, CatalogError> {
        let selector = raw.trim_start_matches("cloud:");
        let mut parts = selector.splitn(2, ':');
        let provider_id = parts.next().unwrap_or_default().trim();
        let requested_model_id = parts.next().unwrap_or_default().trim();

        if provider_id.is_empty() {
            return Err(CatalogError::InvalidSelector(raw.to_string()));
        }

        let provider = self.remote_provider(provider_id).ok_or_else(|| {
            CatalogError::InvalidSelector(format!(
                "{} (unknown remote provider '{}')",
                raw, provider_id
            ))
        })?;
        let model_id = if requested_model_id.is_empty() {
            provider.default_model_id.as_str()
        } else {
            requested_model_id
        };

        let model = provider
            .models
            .iter()
            .find(|model| model.id == model_id)
            .cloned()
            .ok_or_else(|| {
                CatalogError::InvalidSelector(format!(
                    "{} (unsupported model '{}' for provider '{}')",
                    raw, model_id, provider_id
                ))
            })?;

        let runtime_config =
            crate::backend::remote_runtime_config_for_backend(provider.backend_id.as_str())
                .ok_or_else(|| {
                    CatalogError::DriverResolutionFailed(format!(
                        "Missing runtime config for remote backend '{}'.",
                        provider.backend_id
                    ))
                })?;
        if runtime_config.backend_id != provider.backend_id {
            return Err(CatalogError::DriverResolutionFailed(format!(
                "Remote runtime config backend '{}' does not match provider backend '{}'.",
                runtime_config.backend_id, provider.backend_id
            )));
        }
        let tokenizer_path = runtime_config.tokenizer_path.clone();

        let driver_resolution = crate::backend::resolve_driver_for_model(
            PromptFamily::Unknown,
            None,
            Some(provider.backend_id.as_str()),
        )
        .map_err(CatalogError::DriverResolutionFailed)?;

        Ok(ResolvedModelTarget::remote(
            provider.id.clone(),
            provider.label.clone(),
            provider.backend_id.clone(),
            model_id.to_string(),
            model,
            runtime_config,
            tokenizer_path,
            driver_resolution,
        ))
    }

    pub fn format_list_json(&self) -> String {
        if let Some(cached) = self.render_cache.borrow().list_json.clone() {
            return cached;
        }

        let rendered = format_list_json(self);
        self.render_cache.borrow_mut().list_json = Some(rendered.clone());
        rendered
    }

    pub fn format_info_json(&self, model_id: &str) -> Result<String, CatalogError> {
        if let Some(cached) = self.render_cache.borrow().info_json.get(model_id).cloned() {
            return Ok(cached);
        }

        let rendered = format_info_json(self, model_id)?;
        self.render_cache
            .borrow_mut()
            .info_json
            .insert(model_id.to_string(), rendered.clone());
        Ok(rendered)
    }

    pub fn select_for_workload(&self, class: WorkloadClass) -> Option<&ModelEntry> {
        select_for_workload(&self.entries, class)
    }

    fn invalidate_render_cache(&self) {
        *self.render_cache.borrow_mut() = RenderCache::default();
    }
}
