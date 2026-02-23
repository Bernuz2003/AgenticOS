use std::fs;
use std::path::{Path, PathBuf};

use crate::prompting::PromptFamily;

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

        let dir_entries = fs::read_dir(&models_dir)
            .map_err(|e| format!("Model directory read failed '{}': {}", models_dir.display(), e))?;

        for entry in dir_entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
            if extension != "gguf" {
                continue;
            }

            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown-model")
                .to_string();

            let family = infer_family_from_filename(&stem);
            let tokenizer_path = infer_tokenizer_path(&models_dir, &path);

            entries.push(ModelEntry {
                id: stem,
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

        if let Some(entry) = self.find_by_id(raw) {
            return Ok((entry.path.clone(), entry.family));
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

    #[test]
    fn family_inference_from_name() {
        assert_eq!(infer_family_from_filename("Meta-Llama-3-8B"), PromptFamily::Llama);
        assert_eq!(infer_family_from_filename("Qwen2.5-14B"), PromptFamily::Qwen);
        assert_eq!(infer_family_from_filename("Mistral-7B"), PromptFamily::Mistral);
        assert_eq!(infer_family_from_filename("unknown"), PromptFamily::Unknown);
    }
}
