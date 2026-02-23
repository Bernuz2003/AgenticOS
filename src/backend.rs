use anyhow::{Error as E, Result};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::models::quantized_llama;
use candle_transformers::models::quantized_qwen2;

use crate::prompting::PromptFamily;

pub enum RuntimeModel {
    Llama(quantized_llama::ModelWeights),
    Qwen2(quantized_qwen2::ModelWeights),
}

impl RuntimeModel {
    pub fn load_from_gguf(path: &str, family: PromptFamily, device: &Device) -> Result<Self> {
        let mut file = std::fs::File::open(path)
            .map_err(|e| E::msg(format!("Failed to open model file: {}", e)))?;
        let content = gguf_file::Content::read(&mut file)?;

        match family {
            PromptFamily::Llama => {
                let model = quantized_llama::ModelWeights::from_gguf(content, &mut file, device)?;
                Ok(Self::Llama(model))
            }
            PromptFamily::Qwen => {
                match quantized_qwen2::ModelWeights::from_gguf(content, &mut file, device) {
                    Ok(model) => Ok(Self::Qwen2(model)),
                    Err(e) => {
                        let msg = format!("{}", e);
                        if msg.contains("cannot find tensor info for output_norm.weight") {
                            Err(E::msg(
                                "Qwen load failed: missing 'output_norm.weight'. The GGUF is likely an incomplete split shard (or otherwise incomplete export). Use a full single-file GGUF, or merge all split parts before LOAD.",
                            ))
                        } else {
                            Err(E::msg(msg))
                        }
                    }
                }
            }
            PromptFamily::Mistral => Err(E::msg(
                "Mistral backend is not implemented yet in runtime backend."
            )),
            PromptFamily::Unknown => Err(E::msg(
                "Unknown model family: cannot choose runtime backend."
            )),
        }
    }

    pub fn forward(&mut self, input_tensor: &Tensor, position: usize) -> Result<Tensor> {
        match self {
            Self::Llama(model) => Ok(model.forward(input_tensor, position)?),
            Self::Qwen2(model) => Ok(model.forward(input_tensor, position)?),
        }
    }

    pub fn duplicate_if_supported(&self) -> Option<Self> {
        match self {
            Self::Llama(model) => Some(Self::Llama(model.clone())),
            Self::Qwen2(_) => None,
        }
    }
}
