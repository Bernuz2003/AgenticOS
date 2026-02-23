use std::fs;
use std::path::{Path, PathBuf};

use crate::prompting::PromptFamily;

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
}

#[derive(Debug)]
pub struct ModelCatalog {
    pub models_dir: PathBuf,
    pub entries: Vec<ModelEntry>,
    pub selected_id: Option<String>,
}

impl ModelCatalog {
    pub fn discover(models_dir: impl Into<PathBuf>) -> Result<Self, String> {
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

            let family = infer_family_from_filename(&stem);
            let tokenizer_path = infer_tokenizer_path(&models_dir, &path);
            let id = build_model_id(&models_dir, &path);

            entries.push(ModelEntry {
                id,
                path,
                family,
                tokenizer_path,
            });
        }

        entries.sort_by(|a, b| a.id.cmp(&b.id));

        Ok(Self {
            models_dir,
            entries,
            selected_id: None,
        })
    }

    pub fn refresh(&mut self) -> Result<(), String> {
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

    pub fn set_selected(&mut self, model_id: &str) -> Result<(), String> {
        if self.entries.iter().any(|m| m.id == model_id) {
            self.selected_id = Some(model_id.to_string());
            Ok(())
        } else {
            Err(format!("Model '{}' not found in catalog", model_id))
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

    pub fn resolve_load_target(&self, payload: &str) -> Result<(PathBuf, PromptFamily), String> {
        let raw = payload.trim();

        if raw.is_empty() {
            if let Some(entry) = self.selected_entry() {
                return Ok((entry.path.clone(), entry.family));
            }
            return Err("No model selected. Use SELECT_MODEL first or pass a model path/id to LOAD.".to_string());
        }

        if let Some(entry) = self.find_by_id(raw) {
            return Ok((entry.path.clone(), entry.family));
        }

        if raw.ends_with(".gguf") || raw.contains('/') || raw.contains('\\') {
            let path = PathBuf::from(raw);
            if !path.exists() {
                return Err(format!("Model path not found: {}", path.display()));
            }
            let fallback_family = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(infer_family_from_filename)
                .unwrap_or(PromptFamily::Unknown);
            return Ok((path, fallback_family));
        }

        Err(format!(
            "Invalid model selector '{}'. Use model id from LIST_MODELS or provide .gguf path.",
            raw
        ))
    }

    pub fn format_list(&self) -> String {
        if self.entries.is_empty() {
            return "No GGUF models found in models directory.".to_string();
        }

        let selected = self.selected_id.as_deref();
        let mut lines = Vec::with_capacity(self.entries.len() + 2);
        lines.push(format!("Models ({})", self.entries.len()));
        for entry in &self.entries {
            let marker = if selected == Some(entry.id.as_str()) {
                "*"
            } else {
                "-"
            };
            lines.push(format!(
                "{} id={} family={:?} path={}",
                marker,
                entry.id,
                entry.family,
                entry.path.display()
            ));
        }
        lines.join("\n")
    }

    pub fn format_info(&self, model_id: &str) -> Result<String, String> {
        let entry = self
            .find_by_id(model_id)
            .ok_or_else(|| format!("Model '{}' not found", model_id))?;

        Ok(format!(
            "id={}\nfamily={:?}\npath={}\ntokenizer={}",
            entry.id,
            entry.family,
            entry.path.display(),
            entry
                .tokenizer_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<none>".to_string())
        ))
    }

    pub fn select_for_workload(&self, class: WorkloadClass) -> Option<&ModelEntry> {
        if self.entries.is_empty() {
            return None;
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

            if selected.is_some() {
                return selected;
            }
        }

        self.entries.first()
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

fn collect_gguf_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir)
        .map_err(|e| format!("Model directory read failed '{}': {}", dir.display(), e))?;

    for entry in entries {
        let path = entry
            .map_err(|e| format!("Model directory entry read failed '{}': {}", dir.display(), e))?
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

        let (resolved_path, family) = catalog
            .resolve_load_target(&qwen.id)
            .expect("resolve by id with slash");
        assert_eq!(resolved_path, qwen_model);
        assert_eq!(family, PromptFamily::Qwen);

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

    fn mk_temp_dir(prefix: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time ok")
            .as_nanos();
        std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), ts))
    }
}
