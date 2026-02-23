use anyhow::{Error as E, Result};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::models::quantized_llama;

use crate::prompting::PromptFamily;

#[derive(Clone)]
pub enum RuntimeModel {
    Llama(quantized_llama::ModelWeights),
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
            PromptFamily::Qwen => Err(E::msg(
                "Qwen backend is not implemented yet in runtime backend."
            )),
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
        }
    }
}
