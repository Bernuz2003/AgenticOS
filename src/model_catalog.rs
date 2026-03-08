use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use candle_core::quantized::gguf_file;
use serde::{Deserialize, Serialize};

use crate::backend::{resolve_driver_for_model, DriverResolution};
use crate::errors::CatalogError;
use crate::prompting::PromptFamily;

#[derive(Serialize)]
struct ModelListResponse {
    selected_model_id: Option<String>,
    total_models: usize,
    models: Vec<ModelListEntry>,
    routing_recommendations: Vec<RoutingRecommendation>,
}

#[derive(Serialize)]
struct ModelListEntry {
    id: String,
    family: String,
    architecture: Option<String>,
    path: String,
    tokenizer_path: Option<String>,
    tokenizer_present: bool,
    metadata_source: Option<String>,
    backend_preference: Option<String>,
    resolved_backend: Option<String>,
    driver_resolution_source: String,
    driver_resolution_rationale: String,
    driver_available: Option<bool>,
    driver_load_supported: Option<bool>,
    capabilities: Option<std::collections::BTreeMap<String, f64>>,
    selected: bool,
}

#[derive(Serialize)]
struct RoutingRecommendation {
    workload: String,
    model_id: Option<String>,
    family: Option<String>,
    backend_preference: Option<String>,
    resolved_backend: Option<String>,
    driver_resolution_source: String,
    driver_resolution_rationale: String,
    driver_available: Option<bool>,
    driver_load_supported: Option<bool>,
    metadata_source: Option<String>,
    source: String,
    rationale: String,
    capability_key: Option<String>,
    capability_score: Option<f64>,
}

struct RoutingDecision<'a> {
    entry: Option<&'a ModelEntry>,
    source: &'static str,
    rationale: String,
    capability_key: Option<&'static str>,
    capability_score: Option<f64>,
}

#[derive(Serialize)]
struct ModelInfoResponse {
    id: String,
    family: String,
    architecture: Option<String>,
    path: String,
    tokenizer_path: Option<String>,
    tokenizer_present: bool,
    metadata_source: Option<String>,
    backend_preference: Option<String>,
    resolved_backend: Option<String>,
    driver_resolution_source: String,
    driver_resolution_rationale: String,
    driver_available: Option<bool>,
    driver_load_supported: Option<bool>,
    chat_template: Option<String>,
    assistant_preamble: Option<String>,
    special_tokens: Option<std::collections::BTreeMap<String, String>>,
    stop_markers: Option<Vec<String>>,
    capabilities: Option<std::collections::BTreeMap<String, f64>>,
    selected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ModelMetadata {
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub architecture: Option<String>,
    #[serde(default)]
    pub backend_preference: Option<String>,
    #[serde(default)]
    pub chat_template: Option<String>,
    #[serde(default)]
    pub assistant_preamble: Option<String>,
    #[serde(default)]
    pub special_tokens: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub stop_markers: Option<Vec<String>>,
    #[serde(default)]
    pub capabilities: Option<std::collections::BTreeMap<String, f64>>,
}

impl ModelMetadata {
    pub fn declared_family(&self) -> Option<PromptFamily> {
        self.family.as_deref().map(parse_family_label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadClass {
    Fast,
    Code,
    Reasoning,
    General,
}

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
    pub driver_resolution: DriverResolution,
}

#[derive(Debug)]
pub struct ModelCatalog {
    pub models_dir: PathBuf,
    pub entries: Vec<ModelEntry>,
    pub selected_id: Option<String>,
}

struct DriverCatalogView {
    resolved_backend: Option<String>,
    driver_resolution_source: String,
    driver_resolution_rationale: String,
    driver_available: Option<bool>,
    driver_load_supported: Option<bool>,
}

impl ModelCatalog {
    pub fn discover(models_dir: impl Into<PathBuf>) -> Result<Self, CatalogError> {
        let models_dir = models_dir.into();
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
        })
    }

    pub fn refresh(&mut self) -> Result<(), CatalogError> {
        let selected = self.selected_id.clone();
        let mut newer = ModelCatalog::discover(self.models_dir.clone())?;
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
            Ok(())
        } else {
            Err(CatalogError::ModelNotFound(model_id.to_string()))
        }
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
        let selected = self.selected_id.as_deref();
        let payload = ModelListResponse {
            selected_model_id: self.selected_id.clone(),
            total_models: self.entries.len(),
            models: self
                .entries
                .iter()
                .map(|entry| {
                    let driver = driver_view_for_entry(entry);
                    ModelListEntry {
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
                })
                .collect(),
            routing_recommendations: [
                ("fast", WorkloadClass::Fast),
                ("general", WorkloadClass::General),
                ("code", WorkloadClass::Code),
                ("reasoning", WorkloadClass::Reasoning),
            ]
            .into_iter()
            .map(|(workload, class)| {
                let decision = self.recommend_for_workload(class);
                let picked = decision.entry;
                RoutingRecommendation {
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

        serde_json::to_string(&payload).expect("ModelListResponse is serializable")
    }

    pub fn format_info_json(&self, model_id: &str) -> Result<String, CatalogError> {
        let entry = self
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
            selected: self.selected_id.as_deref() == Some(entry.id.as_str()),
        };

        Ok(serde_json::to_string(&payload).expect("ModelInfoResponse is serializable"))
    }

    pub fn select_for_workload(&self, class: WorkloadClass) -> Option<&ModelEntry> {
        self.recommend_for_workload(class).entry
    }

    fn recommend_for_workload(&self, class: WorkloadClass) -> RoutingDecision<'_> {
        if self.entries.is_empty() {
            return RoutingDecision {
                entry: None,
                source: "none",
                rationale: "no models available in catalog".to_string(),
                capability_key: None,
                capability_score: None,
            };
        }

        let capability_key = workload_key(class);
        let mut scored: Vec<(&ModelEntry, f64, usize)> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let score = entry
                    .metadata
                    .as_ref()
                    .and_then(|meta| meta.capabilities.as_ref())
                    .and_then(|caps| caps.get(capability_key))
                    .copied()?;
                Some((entry, score, model_size_hint(&entry.id)))
            })
            .collect();
        if !scored.is_empty() {
            scored.sort_by(|left, right| {
                right
                    .1
                    .partial_cmp(&left.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| left.2.cmp(&right.2))
            });
            if let Some((entry, score, _)) = scored.first().copied() {
                return RoutingDecision {
                    entry: Some(entry),
                    source: "metadata-capability",
                    rationale: format!(
                        "selected by metadata capability '{}' with score {:.2}",
                        capability_key, score
                    ),
                    capability_key: Some(capability_key),
                    capability_score: Some(score),
                };
            }
        }

        let family_order: &[PromptFamily] = match class {
            WorkloadClass::Fast => &[PromptFamily::Llama, PromptFamily::Qwen, PromptFamily::Mistral],
            WorkloadClass::Code => &[PromptFamily::Qwen, PromptFamily::Llama, PromptFamily::Mistral],
            WorkloadClass::Reasoning => {
                &[PromptFamily::Qwen, PromptFamily::Llama, PromptFamily::Mistral]
            }
            WorkloadClass::General => &[PromptFamily::Llama, PromptFamily::Qwen, PromptFamily::Mistral],
        };

        for family in family_order {
            let mut candidates: Vec<&ModelEntry> = self
                .entries
                .iter()
                .filter(|entry| entry.family == *family)
                .collect();

            if candidates.is_empty() {
                continue;
            }

            candidates.sort_by_key(|entry| model_size_hint(&entry.id));

            let selected = match class {
                WorkloadClass::Fast => candidates.first().copied(),
                WorkloadClass::Code | WorkloadClass::Reasoning => candidates.last().copied(),
                WorkloadClass::General => candidates.first().copied(),
            };

            if let Some(entry) = selected {
                return RoutingDecision {
                    entry: Some(entry),
                    source: "family-fallback",
                    rationale: format!(
                        "selected by family fallback '{:?}' for '{}' workload",
                        family, capability_key
                    ),
                    capability_key: None,
                    capability_score: None,
                };
            }
        }

        RoutingDecision {
            entry: self.entries.first(),
            source: "first-available",
            rationale: "selected first available model because no capability or family match applied"
                .to_string(),
            capability_key: None,
            capability_score: None,
        }
    }
}

pub fn infer_workload_class(prompt: &str) -> WorkloadClass {
    let lowered = prompt.to_lowercase();
    if lowered.contains("python")
        || lowered.contains("rust")
        || lowered.contains("codice")
        || lowered.contains("debug")
        || lowered.contains("refactor")
    {
        WorkloadClass::Code
    } else if lowered.contains("ragiona")
        || lowered.contains("reason")
        || lowered.contains("analizza")
        || lowered.contains("dimostra")
    {
        WorkloadClass::Reasoning
    } else if lowered.contains("breve")
        || lowered.contains("short")
        || lowered.contains("riassumi")
        || lowered.contains("ping")
    {
        WorkloadClass::Fast
    } else {
        WorkloadClass::General
    }
}

pub fn parse_workload_hint(prompt: &str) -> (Option<WorkloadClass>, String) {
    let trimmed = prompt.trim_start();
    let lower = trimmed.to_lowercase();
    let prefix = "capability=";

    if !lower.starts_with(prefix) {
        return (None, prompt.to_string());
    }

    let Some(sep_idx) = trimmed.find(';') else {
        return (None, prompt.to_string());
    };

    let hint = trimmed[prefix.len()..sep_idx].trim().to_lowercase();
    let workload = match hint.as_str() {
        "fast" => Some(WorkloadClass::Fast),
        "code" => Some(WorkloadClass::Code),
        "reasoning" => Some(WorkloadClass::Reasoning),
        "general" => Some(WorkloadClass::General),
        _ => None,
    };

    let stripped = trimmed[sep_idx + 1..].trim_start().to_string();
    (workload, stripped)
}

fn model_size_hint(model_id: &str) -> usize {
    let lower = model_id.to_lowercase();
    let mut digits = String::new();
    for ch in lower.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if !digits.is_empty() {
            break;
        }
    }

    if digits.is_empty() {
        0
    } else {
        digits.parse::<usize>().unwrap_or(0)
    }
}

fn infer_family_from_filename(name: &str) -> PromptFamily {
    let lowered = name.to_lowercase();
    if lowered.contains("llama") {
        PromptFamily::Llama
    } else if lowered.contains("qwen") {
        PromptFamily::Qwen
    } else if lowered.contains("mistral") || lowered.contains("mixtral") {
        PromptFamily::Mistral
    } else {
        PromptFamily::Unknown
    }
}

fn parse_family_label(raw: &str) -> PromptFamily {
    let lowered = raw.trim().to_ascii_lowercase();
    if lowered.contains("llama") {
        PromptFamily::Llama
    } else if lowered.contains("qwen") {
        PromptFamily::Qwen
    } else if lowered.contains("mistral") || lowered.contains("mixtral") {
        PromptFamily::Mistral
    } else {
        PromptFamily::Unknown
    }
}

fn family_label(family: PromptFamily) -> Option<String> {
    match family {
        PromptFamily::Llama => Some("Llama".to_string()),
        PromptFamily::Qwen => Some("Qwen".to_string()),
        PromptFamily::Mistral => Some("Mistral".to_string()),
        PromptFamily::Unknown => None,
    }
}

fn workload_key(class: WorkloadClass) -> &'static str {
    match class {
        WorkloadClass::Fast => "fast",
        WorkloadClass::Code => "code",
        WorkloadClass::Reasoning => "reasoning",
        WorkloadClass::General => "general",
    }
}

fn driver_view_for_entry(entry: &ModelEntry) -> DriverCatalogView {
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

fn resolve_driver_for_entry(entry: &ModelEntry) -> Result<DriverResolution, CatalogError> {
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

fn infer_metadata_path(model_path: &Path) -> Option<PathBuf> {
    let parent = model_path.parent()?;
    let sidecar = parent.join("metadata.json");
    if sidecar.exists() {
        return Some(sidecar);
    }

    let stem = model_path.file_stem()?.to_str()?;
    let sibling = parent.join(format!("{}.metadata.json", stem));
    if sibling.exists() {
        return Some(sibling);
    }

    None
}

fn load_model_metadata(path: &Path) -> Option<ModelMetadata> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<ModelMetadata>(&raw).ok()
}

fn load_native_model_metadata(
    model_path: &Path,
    tokenizer_path: Option<&Path>,
) -> (Option<ModelMetadata>, bool, bool) {
    let gguf_metadata = load_gguf_native_metadata(model_path);
    let tokenizer_metadata = tokenizer_path.and_then(load_tokenizer_native_metadata);
    let native_from_gguf = gguf_metadata.is_some();
    let native_from_tokenizer = tokenizer_metadata.is_some();
    (
        merge_model_metadata(gguf_metadata, tokenizer_metadata),
        native_from_gguf,
        native_from_tokenizer,
    )
}

fn load_gguf_native_metadata(path: &Path) -> Option<ModelMetadata> {
    let mut file = fs::File::open(path).ok()?;
    let content = gguf_file::Content::read(&mut file).ok()?;
    parse_gguf_metadata_map(&content.metadata)
}

fn parse_gguf_metadata_map(metadata: &HashMap<String, gguf_file::Value>) -> Option<ModelMetadata> {
    let mut parsed = ModelMetadata::default();

    if let Some(architecture) = metadata
        .get("general.architecture")
        .and_then(|value| value.to_string().ok())
    {
        parsed.architecture = Some(architecture.to_string());
        parsed.family = family_label(parse_family_label(architecture));
    }

    if let Some(template) = metadata
        .get("tokenizer.chat_template")
        .and_then(|value| value.to_string().ok())
        .cloned()
    {
        if !template.trim().is_empty() {
            parsed.chat_template = Some(template);
        }
    }

    if parsed == ModelMetadata::default() {
        None
    } else {
        Some(parsed)
    }
}

fn load_tokenizer_native_metadata(path: &Path) -> Option<ModelMetadata> {
    let raw = fs::read_to_string(path).ok()?;
    parse_tokenizer_metadata_json(&raw)
}

fn parse_tokenizer_metadata_json(raw: &str) -> Option<ModelMetadata> {
    let json: serde_json::Value = serde_json::from_str(raw).ok()?;
    let added_tokens = json.get("added_tokens")?.as_array()?;

    let mut special_tokens = BTreeMap::new();
    for token in added_tokens {
        let content = token.get("content")?.as_str()?;
        let is_special = token
            .get("special")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if !is_special {
            continue;
        }

        insert_special_token_aliases(&mut special_tokens, content);
    }

    if special_tokens.is_empty() {
        return None;
    }

    let mut stop_markers = Vec::new();
    for key in ["eos", "end_of_text", "eot", "im_end", "assistant_end"] {
        if let Some(value) = special_tokens.get(key) {
            if !stop_markers.iter().any(|existing| existing == value) {
                stop_markers.push(value.clone());
            }
        }
    }

    let mut parsed = ModelMetadata::default();
    parsed.special_tokens = Some(special_tokens);
    if !stop_markers.is_empty() {
        parsed.stop_markers = Some(stop_markers);
    }
    Some(parsed)
}

fn insert_special_token_aliases(tokens: &mut BTreeMap<String, String>, content: &str) {
    let lowered = content.to_ascii_lowercase();

    if lowered.contains("begin_of_text") {
        tokens.insert("bos".to_string(), content.to_string());
    }
    if lowered.contains("end_of_text") || lowered.contains("endoftext") {
        tokens.insert("eos".to_string(), content.to_string());
        tokens.insert("end_of_text".to_string(), content.to_string());
    }
    if lowered.contains("eot") {
        tokens.insert("eot".to_string(), content.to_string());
    }
    if lowered.contains("im_start") {
        tokens.insert("im_start".to_string(), content.to_string());
    }
    if lowered.contains("im_end") {
        tokens.insert("im_end".to_string(), content.to_string());
        tokens.insert("assistant_end".to_string(), content.to_string());
        tokens.insert("eot".to_string(), content.to_string());
    }
    if lowered.contains("start_header_id") {
        tokens.insert("start_header_id".to_string(), content.to_string());
    }
    if lowered.contains("end_header_id") {
        tokens.insert("end_header_id".to_string(), content.to_string());
    }
}

fn merge_model_metadata(
    base: Option<ModelMetadata>,
    overlay: Option<ModelMetadata>,
) -> Option<ModelMetadata> {
    match (base, overlay) {
        (None, None) => None,
        (Some(metadata), None) | (None, Some(metadata)) => Some(metadata),
        (Some(mut base), Some(overlay)) => {
            if overlay.family.is_some() {
                base.family = overlay.family;
            }
            if overlay.architecture.is_some() {
                base.architecture = overlay.architecture;
            }
            if overlay.backend_preference.is_some() {
                base.backend_preference = overlay.backend_preference;
            }
            if overlay.chat_template.is_some() {
                base.chat_template = overlay.chat_template;
            }
            if overlay.assistant_preamble.is_some() {
                base.assistant_preamble = overlay.assistant_preamble;
            }
            if overlay.special_tokens.is_some() {
                base.special_tokens = overlay.special_tokens;
            }
            if overlay.stop_markers.is_some() {
                base.stop_markers = overlay.stop_markers;
            }
            if overlay.capabilities.is_some() {
                base.capabilities = overlay.capabilities;
            }
            Some(base)
        }
    }
}

fn describe_metadata_source(
    native_from_gguf: bool,
    native_from_tokenizer: bool,
    sidecar_path: Option<&Path>,
    metadata: Option<&ModelMetadata>,
) -> Option<String> {
    if metadata.is_none() {
        return None;
    }

    let mut native_parts = Vec::new();
    if native_from_gguf {
        native_parts.push("gguf");
    }
    if native_from_tokenizer {
        native_parts.push("tokenizer");
    }

    let native_label = if native_parts.is_empty() {
        None
    } else {
        Some(format!("native:{}", native_parts.join("+")))
    };

    match (native_label, sidecar_path) {
        (Some(native), Some(path)) => Some(format!("{}+sidecar:{}", native, path.display())),
        (Some(native), None) => Some(native),
        (None, Some(path)) => Some(path.display().to_string()),
        (None, None) => None,
    }
}

fn infer_tokenizer_path(models_dir: &Path, model_path: &Path) -> Option<PathBuf> {
    let model_parent = model_path.parent().unwrap_or(models_dir);
    let local_tok = model_parent.join("tokenizer.json");
    if local_tok.exists() {
        return Some(local_tok);
    }

    let root_tok = PathBuf::from("tokenizer.json");
    if root_tok.exists() {
        return Some(root_tok);
    }

    let models_tok = models_dir.join("tokenizer.json");
    if models_tok.exists() {
        return Some(models_tok);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
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
