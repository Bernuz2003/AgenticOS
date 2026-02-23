use std::path::{Path, PathBuf};

use tokenizers::Tokenizer;

use crate::prompting::PromptFamily;

pub(super) fn resolve_tokenizer_path(model_path: &str, tokenizer_hint: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(hint) = tokenizer_hint {
        if hint.exists() {
            return Some(hint);
        }
    }

    let model_path = Path::new(model_path);
    let parent_dir = model_path.parent().unwrap_or(Path::new("."));
    let local_tok_path = parent_dir.join("tokenizer.json");
    if local_tok_path.exists() {
        return Some(local_tok_path);
    }

    let root_tok_path = Path::new("tokenizer.json");
    if root_tok_path.exists() {
        return Some(root_tok_path.to_path_buf());
    }

    let models_tok_path = Path::new("models").join("tokenizer.json");
    if models_tok_path.exists() {
        return Some(models_tok_path);
    }

    None
}

pub(super) fn resolve_special_tokens(
    tokenizer: &Tokenizer,
    family: PromptFamily,
) -> Result<(u32, u32), String> {
    match family {
        PromptFamily::Llama => {
            let eos = tokenizer
                .token_to_id("<|end_of_text|>")
                .or_else(|| tokenizer.token_to_id("</s>"))
                .ok_or_else(|| {
                    "Tokenizer/model incompatibility: Llama requires <|end_of_text|> or </s>."
                        .to_string()
                })?;

            let eot = tokenizer
                .token_to_id("<|eot_id|>")
                .ok_or_else(|| {
                    "Tokenizer/model incompatibility: Llama template requires <|eot_id|>."
                        .to_string()
                })?;

            let has_headers = tokenizer.token_to_id("<|start_header_id|>").is_some()
                && tokenizer.token_to_id("<|end_header_id|>").is_some();
            if !has_headers {
                return Err(
                    "Tokenizer/model incompatibility: missing Llama chat header tokens (<|start_header_id|>, <|end_header_id|>).".to_string(),
                );
            }

            Ok((eos, eot))
        }
        PromptFamily::Qwen => {
            let eos = tokenizer
                .token_to_id("<|endoftext|>")
                .or_else(|| tokenizer.token_to_id("</s>"))
                .ok_or_else(|| {
                    "Tokenizer/model incompatibility: Qwen requires <|endoftext|> or </s>."
                        .to_string()
                })?;

            let eot = tokenizer
                .token_to_id("<|im_end|>")
                .ok_or_else(|| {
                    "Tokenizer/model incompatibility: Qwen template requires <|im_end|>."
                        .to_string()
                })?;

            if tokenizer.token_to_id("<|im_start|>").is_none() {
                return Err(
                    "Tokenizer/model incompatibility: Qwen template requires <|im_start|>."
                        .to_string(),
                );
            }

            Ok((eos, eot))
        }
        PromptFamily::Mistral => {
            let eos = tokenizer
                .token_to_id("</s>")
                .or_else(|| tokenizer.token_to_id("<|end_of_text|>"))
                .ok_or_else(|| {
                    "Tokenizer/model incompatibility: Mistral requires </s> or <|end_of_text|>."
                        .to_string()
                })?;
            Ok((eos, eos))
        }
        PromptFamily::Unknown => {
            let eos = tokenizer
                .token_to_id("<|end_of_text|>")
                .or_else(|| tokenizer.token_to_id("</s>"))
                .or_else(|| tokenizer.token_to_id("<|endoftext|>"))
                .unwrap_or(2);
            Ok((eos, eos))
        }
    }
}
