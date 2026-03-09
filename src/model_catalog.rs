use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::errors::CatalogError;
use crate::prompting::PromptFamily;
mod formatting;
mod metadata;
mod routing;
mod workload;

pub use metadata::ModelMetadata;
pub use workload::{infer_workload_class, parse_workload_hint, parse_workload_label, WorkloadClass};

use formatting::{format_info_json, format_list_json};
use metadata::{
    describe_metadata_source, infer_family_from_filename, infer_metadata_path,
    infer_tokenizer_path, load_model_metadata, load_native_model_metadata,
    merge_model_metadata,
};
use routing::{resolve_driver_for_entry, select_for_workload};

#[cfg(test)]
use metadata::{parse_gguf_metadata_map, parse_tokenizer_metadata_json};

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

#[derive(Debug, Default)]
struct RenderCache {
    list_json: Option<String>,
    info_json: HashMap<String, String>,
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
        let mut entries = Vec::new();

        let mut gguf_files = Vec::new();
        collect_gguf_files(&models_dir, &mut gguf_files)?;

        for path in gguf_files {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown-model")
                .to_string();

            let tokenizer_path = infer_tokenizer_path(&models_dir, &path);
            let (native_metadata, native_from_gguf, native_from_tokenizer) =
                load_native_model_metadata(&path, tokenizer_path.as_deref());
            let metadata_path = infer_metadata_path(&path);
            let sidecar_metadata = metadata_path
                .as_ref()
                .and_then(|meta_path| load_model_metadata(meta_path));
            let metadata = merge_model_metadata(native_metadata, sidecar_metadata);
            let family = metadata
                .as_ref()
                .and_then(ModelMetadata::declared_family)
                .unwrap_or_else(|| infer_family_from_filename(&stem));
            let id = build_model_id(&models_dir, &path);
            let metadata_source = describe_metadata_source(
                native_from_gguf,
                native_from_tokenizer,
                metadata_path.as_deref(),
                metadata.as_ref(),
            );

            entries.push(ModelEntry {
                id,
                path,
                family,
                tokenizer_path,
                metadata_source,
                metadata,
            });
        }

        entries.sort_by(|a, b| a.id.cmp(&b.id));

        Ok(Self {
            models_dir,
            entries,
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
            if newer.entries.iter().any(|e| e.id == sel) {
                newer.selected_id = Some(sel);
            }
        }
        *self = newer;
        Ok(())
    }

    pub fn set_selected(&mut self, model_id: &str) -> Result<(), CatalogError> {
        if self.entries.iter().any(|m| m.id == model_id) {
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
            .and_then(|id| self.entries.iter().find(|m| &m.id == id))
    }

    pub fn find_by_id(&self, model_id: &str) -> Option<&ModelEntry> {
        self.entries.iter().find(|m| m.id == model_id)
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
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown-model")
            .to_string();
        let tokenizer_path = infer_tokenizer_path(&self.models_dir, &path);
        let (native_metadata, native_from_gguf, native_from_tokenizer) =
            load_native_model_metadata(&path, tokenizer_path.as_deref());
        let metadata_path = infer_metadata_path(&path);
        let sidecar_metadata = metadata_path
            .as_ref()
            .and_then(|meta_path| load_model_metadata(meta_path));
        let metadata = merge_model_metadata(native_metadata, sidecar_metadata);
        let family = metadata
            .as_ref()
            .and_then(ModelMetadata::declared_family)
            .unwrap_or_else(|| infer_family_from_filename(&stem));
        let entry = ModelEntry {
            id: build_model_id(&self.models_dir, &path),
            path: path.clone(),
            family,
            tokenizer_path: tokenizer_path.clone(),
            metadata_source: describe_metadata_source(
                native_from_gguf,
                native_from_tokenizer,
                metadata_path.as_deref(),
                metadata.as_ref(),
            ),
            metadata: metadata.clone(),
        };

        Ok(ResolvedModelTarget {
            model_id: None,
            path,
            family,
            tokenizer_path,
            metadata,
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

fn compute_catalog_fingerprint(models_dir: &Path) -> Result<u64, CatalogError> {
    let mut files = Vec::new();
    collect_catalog_signature_files(models_dir, &mut files)?;
    files.sort();

    let mut hasher = DefaultHasher::new();
    for path in files {
        let metadata = fs::metadata(&path).map_err(|e| CatalogError::DirectoryReadFailed {
            path: path.display().to_string(),
            detail: e.to_string(),
        })?;
        let relative = path.strip_prefix(models_dir).unwrap_or(path.as_path());
        relative.to_string_lossy().hash(&mut hasher);
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
    }

    Ok(hasher.finish())
}

fn collect_catalog_signature_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), CatalogError> {
    let entries = fs::read_dir(dir).map_err(|e| CatalogError::DirectoryReadFailed {
        path: dir.display().to_string(),
        detail: e.to_string(),
    })?;

    for entry in entries {
        let path = entry
            .map_err(|e| CatalogError::DirectoryReadFailed {
                path: dir.display().to_string(),
                detail: e.to_string(),
            })?
            .path();

        if path.is_dir() {
            collect_catalog_signature_files(&path, out)?;
            continue;
        }

        if !path.is_file() || !is_catalog_relevant_file(&path) {
            continue;
        }

        out.push(path);
    }

    Ok(())
}

fn is_catalog_relevant_file(path: &Path) -> bool {
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or_default();

    extension.eq_ignore_ascii_case("gguf")
        || file_name.eq_ignore_ascii_case("tokenizer.json")
        || file_name.eq_ignore_ascii_case("metadata.json")
        || file_name.ends_with(".metadata.json")
}

fn collect_gguf_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), CatalogError> {
    let entries = fs::read_dir(dir)
        .map_err(|e| CatalogError::DirectoryReadFailed {
            path: dir.display().to_string(),
            detail: e.to_string(),
        })?;

    for entry in entries {
        let path = entry
            .map_err(|e| CatalogError::DirectoryReadFailed {
                path: dir.display().to_string(),
                detail: e.to_string(),
            })?
            .path();

        if path.is_dir() {
            collect_gguf_files(&path, out)?;
            continue;
        }

        if !path.is_file() {
            continue;
        }

        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        if extension.eq_ignore_ascii_case("gguf") {
            out.push(path);
        }
    }

    Ok(())
}

fn build_model_id(models_dir: &Path, model_path: &Path) -> String {
    let relative = model_path
        .strip_prefix(models_dir)
        .unwrap_or(model_path)
        .to_path_buf();
    let mut without_ext = relative;
    without_ext.set_extension("");
    without_ext.to_string_lossy().replace('\\', "/")
}


#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::quantized::gguf_file;
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn family_inference_from_name() {
        assert_eq!(infer_family_from_filename("Meta-Llama-3-8B"), PromptFamily::Llama);
        assert_eq!(infer_family_from_filename("Qwen2.5-14B"), PromptFamily::Qwen);
        assert_eq!(infer_family_from_filename("Mistral-7B"), PromptFamily::Mistral);
        assert_eq!(infer_family_from_filename("unknown"), PromptFamily::Unknown);
    }

    #[test]
    fn discovers_models_recursively_in_family_subdirs() {
        let base = mk_temp_dir("agenticos_catalog_recursive");
        let models = base.join("models");
        let llama_dir = models.join("llama3.1-8b");
        let qwen_dir = models.join("qwen2.5-14b");

        fs::create_dir_all(&llama_dir).expect("create llama dir");
        fs::create_dir_all(&qwen_dir).expect("create qwen dir");

        let llama_model = llama_dir.join("Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf");
        let qwen_model = qwen_dir.join("Qwen2.5-14B-Instruct-Q4_K_M.gguf");
        fs::write(&llama_model, b"stub").expect("write llama stub");
        fs::write(&qwen_model, b"stub").expect("write qwen stub");
        fs::write(llama_dir.join("tokenizer.json"), b"{}").expect("write llama tokenizer");

        let catalog = ModelCatalog::discover(&models).expect("discover models recursively");
        assert_eq!(catalog.entries.len(), 2);

        let llama = catalog
            .entries
            .iter()
            .find(|e| e.family == PromptFamily::Llama)
            .expect("llama entry present");
        assert!(llama.id.contains("llama3.1-8b/Meta-Llama-3.1-8B-Instruct-Q4_K_M"));
        assert!(llama
            .tokenizer_path
            .as_ref()
            .expect("tokenizer expected")
            .ends_with("tokenizer.json"));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn metadata_sidecar_overrides_family_and_exposes_capabilities() {
        let base = mk_temp_dir("agenticos_catalog_metadata");
        let models = base.join("models");
        let qwen_dir = models.join("future-model");

        fs::create_dir_all(&qwen_dir).expect("create model dir");
        let model = qwen_dir.join("custom-model.gguf");
        fs::write(&model, b"stub").expect("write model");
        fs::write(
            qwen_dir.join("metadata.json"),
            r#"{
                "family": "Qwen",
                "backend_preference": "external-llamacpp",
                "capabilities": { "code": 0.95, "general": 0.25 }
            }"#,
        )
        .expect("write metadata");

        let catalog = ModelCatalog::discover(&models).expect("discover with metadata");
        let entry = catalog.entries.first().expect("one entry");
        assert_eq!(entry.family, PromptFamily::Qwen);
        assert_eq!(
            entry.metadata.as_ref().and_then(|meta| meta.backend_preference.as_deref()),
            Some("external-llamacpp")
        );
        assert!(entry
            .metadata_source
            .as_deref()
            .unwrap_or_default()
            .ends_with("metadata.json"));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn parse_gguf_metadata_extracts_architecture_and_template() {
        let mut metadata = HashMap::new();
        metadata.insert(
            "general.architecture".to_string(),
            gguf_file::Value::String("qwen2".to_string()),
        );
        metadata.insert(
            "tokenizer.chat_template".to_string(),
            gguf_file::Value::String("<{role}>{content}</{role}>".to_string()),
        );

        let parsed = parse_gguf_metadata_map(&metadata).expect("gguf metadata parsed");
        assert_eq!(parsed.family.as_deref(), Some("Qwen"));
        assert_eq!(parsed.architecture.as_deref(), Some("qwen2"));
        assert_eq!(parsed.chat_template.as_deref(), Some("<{role}>{content}</{role}>"));
    }

    #[test]
    fn parse_tokenizer_metadata_extracts_special_tokens() {
        let parsed = parse_tokenizer_metadata_json(
            r#"{
                "added_tokens": [
                    {"content": "<|endoftext|>", "special": true},
                    {"content": "<|im_start|>", "special": true},
                    {"content": "<|im_end|>", "special": true}
                ]
            }"#,
        )
        .expect("tokenizer metadata parsed");

        let special_tokens = parsed.special_tokens.expect("special tokens present");
        assert_eq!(special_tokens.get("eos").map(String::as_str), Some("<|endoftext|>"));
        assert_eq!(special_tokens.get("im_end").map(String::as_str), Some("<|im_end|>"));
        assert!(parsed
            .stop_markers
            .as_ref()
            .is_some_and(|markers| markers.iter().any(|marker| marker == "<|im_end|>")));
    }

    #[test]
    fn discover_uses_native_tokenizer_metadata_without_sidecar() {
        let base = mk_temp_dir("agenticos_catalog_native_tokenizer");
        let models = base.join("models");
        let qwen_dir = models.join("qwen2.5-14b");

        fs::create_dir_all(&qwen_dir).expect("create qwen dir");
        fs::write(qwen_dir.join("qwen.gguf"), b"stub").expect("write gguf stub");
        fs::write(
            qwen_dir.join("tokenizer.json"),
            r#"{
                "added_tokens": [
                    {"content": "<|endoftext|>", "special": true},
                    {"content": "<|im_start|>", "special": true},
                    {"content": "<|im_end|>", "special": true}
                ]
            }"#,
        )
        .expect("write tokenizer");

        let catalog = ModelCatalog::discover(&models).expect("discover models");
        let entry = catalog.entries.first().expect("entry present");
        assert_eq!(entry.family, PromptFamily::Qwen);
        assert_eq!(
            entry
                .metadata
                .as_ref()
                .and_then(|meta| meta.special_tokens.as_ref())
                .and_then(|tokens| tokens.get("im_end"))
                .map(String::as_str),
            Some("<|im_end|>")
        );
        assert_eq!(entry.metadata_source.as_deref(), Some("native:tokenizer"));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn metadata_capabilities_drive_routing_before_family_heuristics() {
        let base = mk_temp_dir("agenticos_catalog_routing");
        let models = base.join("models");
        let llama_dir = models.join("llama3.1-8b");
        let qwen_dir = models.join("qwen2.5-14b");

        fs::create_dir_all(&llama_dir).expect("create llama dir");
        fs::create_dir_all(&qwen_dir).expect("create qwen dir");
        fs::write(llama_dir.join("llama.gguf"), b"stub").expect("write llama");
        fs::write(qwen_dir.join("qwen.gguf"), b"stub").expect("write qwen");
        fs::write(
            llama_dir.join("metadata.json"),
            r#"{ "family": "Llama", "capabilities": { "code": 0.2 } }"#,
        )
        .expect("write llama metadata");
        fs::write(
            qwen_dir.join("metadata.json"),
            r#"{ "family": "Qwen", "capabilities": { "code": 0.9 } }"#,
        )
        .expect("write qwen metadata");

        let catalog = ModelCatalog::discover(&models).expect("discover models");
        let selected = catalog
            .select_for_workload(WorkloadClass::Code)
            .expect("select code model");
        assert_eq!(selected.family, PromptFamily::Qwen);

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn resolve_load_target_prefers_model_id_even_if_contains_slash() {
        let base = mk_temp_dir("agenticos_catalog_id");
        let models = base.join("models");
        let qwen_dir = models.join("qwen2.5-14b");

        fs::create_dir_all(&qwen_dir).expect("create qwen dir");
        let qwen_model = qwen_dir.join("Qwen2.5-14B-Instruct-Q4_K_M.gguf");
        fs::write(&qwen_model, b"stub").expect("write qwen stub");

        let catalog = ModelCatalog::discover(&models).expect("discover models");
        let qwen = catalog
            .entries
            .iter()
            .find(|e| e.family == PromptFamily::Qwen)
            .expect("qwen entry present");

        let target = catalog
            .resolve_load_target(&qwen.id)
            .expect("resolve by id with slash");
        assert_eq!(target.path, qwen_model);
        assert_eq!(target.family, PromptFamily::Qwen);
        assert_eq!(target.model_id.as_deref(), Some(qwen.id.as_str()));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn parse_and_infer_workload() {
        let (hint, stripped) = parse_workload_hint("capability=code; scrivi parser rust");
        assert_eq!(hint, Some(WorkloadClass::Code));
        assert_eq!(stripped, "scrivi parser rust");

        assert_eq!(infer_workload_class("rispondi breve"), WorkloadClass::Fast);
        assert_eq!(infer_workload_class("ragiona su questo problema"), WorkloadClass::Reasoning);
    }

    #[test]
    fn format_list_json_exposes_models_and_routing() {
        let base = mk_temp_dir("agenticos_catalog_json");
        let models = base.join("models");
        let llama_dir = models.join("llama3.1-8b");
        let qwen_dir = models.join("qwen2.5-14b");

        fs::create_dir_all(&llama_dir).expect("create llama dir");
        fs::create_dir_all(&qwen_dir).expect("create qwen dir");
        fs::write(llama_dir.join("meta-llama-3.1-8b.gguf"), b"stub").expect("write llama stub");
        fs::write(qwen_dir.join("qwen2.5-14b.gguf"), b"stub").expect("write qwen stub");
        fs::write(qwen_dir.join("tokenizer.json"), b"{}").expect("write tokenizer");

        let mut catalog = ModelCatalog::discover(&models).expect("discover models");
        let first_id = catalog.entries[0].id.clone();
        catalog.set_selected(&first_id).expect("select first model");

        let payload: serde_json::Value = serde_json::from_str(&catalog.format_list_json()).expect("json payload");
        assert_eq!(payload["total_models"].as_u64(), Some(2));
        assert!(payload["models"].as_array().map(|v| !v.is_empty()).unwrap_or(false));
        assert!(payload["routing_recommendations"].as_array().map(|v| !v.is_empty()).unwrap_or(false));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn format_list_json_exposes_routing_source_and_score() {
        let base = mk_temp_dir("agenticos_catalog_routing_meta_json");
        let models = base.join("models");
        let qwen_dir = models.join("future-model");

        fs::create_dir_all(&qwen_dir).expect("create model dir");
        fs::write(qwen_dir.join("custom.gguf"), b"stub").expect("write model");
        fs::write(
            qwen_dir.join("metadata.json"),
            r#"{
                "family": "Qwen",
                "backend_preference": "external-llamacpp",
                "capabilities": { "code": 0.93 }
            }"#,
        )
        .expect("write metadata");

        let catalog = ModelCatalog::discover(&models).expect("discover models");
        let payload: serde_json::Value =
            serde_json::from_str(&catalog.format_list_json()).expect("json payload");
        let code_route = payload["routing_recommendations"]
            .as_array()
            .and_then(|items| {
                items.iter().find(|item| item["workload"].as_str() == Some("code"))
            })
            .expect("code route present");

        assert_eq!(code_route["source"].as_str(), Some("metadata-capability"));
        assert_eq!(code_route["capability_key"].as_str(), Some("code"));
        assert_eq!(code_route["backend_preference"].as_str(), Some("external-llamacpp"));
        assert_eq!(
            code_route["resolved_backend"].as_str(),
            Some("candle.quantized_qwen2")
        );
        assert_eq!(
            code_route["driver_resolution_source"].as_str(),
            Some("metadata-preference-fallback")
        );
        assert_eq!(code_route["capability_score"].as_f64(), Some(0.93));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn format_info_json_exposes_unresolved_driver_when_no_loadable_backend_exists() {
        let base = mk_temp_dir("agenticos_catalog_driver_info");
        let models = base.join("models");
        let mistral_dir = models.join("mistral-7b");

        fs::create_dir_all(&mistral_dir).expect("create mistral dir");
        fs::write(mistral_dir.join("mistral.gguf"), b"stub").expect("write model");
        fs::write(
            mistral_dir.join("metadata.json"),
            r#"{ "family": "Mistral" }"#,
        )
        .expect("write metadata");

        let catalog = ModelCatalog::discover(&models).expect("discover models");
        let info: serde_json::Value = serde_json::from_str(
            &catalog
                .format_info_json("mistral-7b/mistral")
                .expect("model info"),
        )
        .expect("json info");

        assert_eq!(info["resolved_backend"], serde_json::Value::Null);
        assert_eq!(info["driver_resolution_source"].as_str(), Some("unresolved"));
        assert!(info["driver_resolution_rationale"]
            .as_str()
            .unwrap_or_default()
            .contains("No registered loadable driver"));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn format_info_json_exposes_unresolved_driver_for_unsupported_architecture() {
        let base = mk_temp_dir("agenticos_catalog_qwen35_driver_info");
        let models = base.join("models");
        let qwen_dir = models.join("qwen3.5-9b");

        fs::create_dir_all(&qwen_dir).expect("create qwen dir");
        fs::write(qwen_dir.join("model.gguf"), b"stub").expect("write model");
        fs::write(
            qwen_dir.join("metadata.json"),
            r#"{ "family": "Qwen", "architecture": "qwen35" }"#,
        )
        .expect("write metadata");

        let catalog = ModelCatalog::discover(&models).expect("discover models");
        let info: serde_json::Value = serde_json::from_str(
            &catalog
                .format_info_json("qwen3.5-9b/model")
                .expect("model info"),
        )
        .expect("json info");

        assert_eq!(info["architecture"].as_str(), Some("qwen35"));
        assert_eq!(info["resolved_backend"], serde_json::Value::Null);
        assert_eq!(info["driver_resolution_source"].as_str(), Some("unresolved"));
        assert!(info["driver_resolution_rationale"]
            .as_str()
            .unwrap_or_default()
            .contains("qwen35"));

        let _ = fs::remove_dir_all(base);
    }

    fn mk_temp_dir(prefix: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time ok")
            .as_nanos();
        std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), ts))
    }
}
