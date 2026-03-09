use std::cell::RefCell;
use std::path::PathBuf;

use crate::errors::CatalogError;
use crate::prompting::PromptFamily;

mod cache;
mod discovery;
mod driver;
mod formatting;
mod metadata;
mod routing;
#[cfg(test)]
mod tests;
mod workload;

pub use metadata::ModelMetadata;
pub use workload::{infer_workload_class, parse_workload_hint, parse_workload_label, WorkloadClass};

use cache::RenderCache;
use discovery::{build_entry, compute_catalog_fingerprint, discover_entries};
use driver::resolve_driver_for_entry;
use formatting::{format_info_json, format_list_json};
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
pub struct ResolvedModelTarget {
    pub model_id: Option<String>,
    pub path: PathBuf,
    pub family: PromptFamily,
    pub tokenizer_path: Option<PathBuf>,
    pub metadata: Option<ModelMetadata>,
    pub driver_resolution: crate::backend::DriverResolution,
}

#[derive(Debug)]
pub struct ModelCatalog {
    pub models_dir: PathBuf,
    pub entries: Vec<ModelEntry>,
    pub selected_id: Option<String>,
    catalog_fingerprint: u64,
    render_cache: RefCell<RenderCache>,
}

impl ModelCatalog {
    pub fn discover(models_dir: impl Into<PathBuf>) -> Result<Self, CatalogError> {
        let models_dir = models_dir.into();
        let catalog_fingerprint = compute_catalog_fingerprint(&models_dir)?;
        Self::discover_with_fingerprint(models_dir, catalog_fingerprint)
    }

    fn discover_with_fingerprint(
        models_dir: PathBuf,
        catalog_fingerprint: u64,
    ) -> Result<Self, CatalogError> {
        Ok(Self {
            entries: discover_entries(&models_dir)?,
            models_dir,
            selected_id: None,
            catalog_fingerprint,
            render_cache: RefCell::new(RenderCache::default()),
        })
    }

    pub fn refresh(&mut self) -> Result<(), CatalogError> {
        let latest_fingerprint = compute_catalog_fingerprint(&self.models_dir)?;
        if latest_fingerprint == self.catalog_fingerprint {
            return Ok(());
        }

        let selected = self.selected_id.clone();
        let mut newer = ModelCatalog::discover_with_fingerprint(
            self.models_dir.clone(),
            latest_fingerprint,
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

    pub fn resolve_load_target(&self, payload: &str) -> Result<ResolvedModelTarget, CatalogError> {
        let raw = payload.trim();

        if raw.is_empty() {
            if let Some(entry) = self.selected_entry() {
                return self.resolve_entry_target(entry);
            }
            return Err(CatalogError::NoModelSelected);
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
        Ok(ResolvedModelTarget {
            model_id: Some(entry.id.clone()),
            path: entry.path.clone(),
            family: entry.family,
            tokenizer_path: entry.tokenizer_path.clone(),
            metadata: entry.metadata.clone(),
            driver_resolution: resolve_driver_for_entry(entry)?,
        })
    }

    fn resolve_path_target(&self, path: PathBuf) -> Result<ResolvedModelTarget, CatalogError> {
        let entry = build_entry(&self.models_dir, path.clone());

        Ok(ResolvedModelTarget {
            model_id: None,
            path,
            family: entry.family,
            tokenizer_path: entry.tokenizer_path.clone(),
            metadata: entry.metadata.clone(),
            driver_resolution: resolve_driver_for_entry(&entry)?,
        })
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