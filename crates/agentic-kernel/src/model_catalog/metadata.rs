use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::prompting::PromptFamily;

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
    pub special_tokens: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub stop_markers: Option<Vec<String>>,
    #[serde(default)]
    pub capabilities: Option<BTreeMap<String, f64>>,
}

impl ModelMetadata {
    pub fn declared_family(&self) -> Option<PromptFamily> {
        self.family.as_deref().map(parse_family_label)
    }
}

pub(super) fn infer_metadata_path(model_path: &Path) -> Option<PathBuf> {
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

pub(super) fn load_model_metadata(path: &Path) -> Option<ModelMetadata> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<ModelMetadata>(&raw).ok()
}

pub(super) fn load_native_model_metadata(
    _model_path: &Path,
    tokenizer_path: Option<&Path>,
) -> (Option<ModelMetadata>, bool, bool) {
    let tokenizer_metadata = tokenizer_path.and_then(load_tokenizer_native_metadata);
    let native_from_tokenizer = tokenizer_metadata.is_some();
    (tokenizer_metadata, false, native_from_tokenizer)
}

fn load_tokenizer_native_metadata(path: &Path) -> Option<ModelMetadata> {
    let raw = fs::read_to_string(path).ok()?;
    parse_tokenizer_metadata_json(&raw)
}

pub(super) fn parse_tokenizer_metadata_json(raw: &str) -> Option<ModelMetadata> {
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

    let mut parsed = ModelMetadata {
        special_tokens: Some(special_tokens),
        ..ModelMetadata::default()
    };
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

pub(super) fn merge_model_metadata(
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

pub(super) fn describe_metadata_source(
    native_from_gguf: bool,
    native_from_tokenizer: bool,
    sidecar_path: Option<&Path>,
    metadata: Option<&ModelMetadata>,
) -> Option<String> {
    metadata?;

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

pub(super) fn infer_tokenizer_path(models_dir: &Path, model_path: &Path) -> Option<PathBuf> {
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

pub(super) fn infer_family_from_filename(name: &str) -> PromptFamily {
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

pub(super) fn parse_family_label(raw: &str) -> PromptFamily {
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
