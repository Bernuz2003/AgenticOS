use anyhow::{Error as E, Result};
use candle_core::quantized::gguf_file;
use candle_core::{DType, Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_llama;
use candle_transformers::models::quantized_qwen2;
use tokenizers::Tokenizer;

use crate::memory::ContextSlotId;
use crate::prompting::{GenerationConfig, PromptFamily};

use super::{ContextSlotPersistence, InferenceBackend, InferenceStepResult, ModelBackend};

pub(crate) struct QuantizedLlamaBackend {
    weights: quantized_llama::ModelWeights,
}

impl QuantizedLlamaBackend {
    pub(crate) fn load(path: &str, device: &Device) -> Result<Self> {
        let mut file = std::fs::File::open(path)
            .map_err(|e| E::msg(format!("Failed to open model file: {}", e)))?;
        let content = gguf_file::Content::read(&mut file)?;
        let weights = quantized_llama::ModelWeights::from_gguf(content, &mut file, device)?;
        Ok(Self { weights })
    }
}

fn generate_local_step<F>(
    _context_slot_id: Option<ContextSlotId>,
    tokens: &[u32],
    index_pos: usize,
    logits_processor: &mut LogitsProcessor,
    tokenizer: &Tokenizer,
    generation: GenerationConfig,
    device: &Device,
    eos_token_id: u32,
    eot_token_id: u32,
    mut forward: F,
) -> Result<InferenceStepResult>
where
    F: FnMut(&Tensor, usize) -> Result<Tensor>,
{
    let mut next_token: Option<u32> = None;
    let mut cursor = index_pos;

    while cursor < tokens.len() {
        let input_token = tokens[cursor];
        let input_tensor = Tensor::new(&[input_token], device)?.unsqueeze(0)?;
        let logits = forward(&input_tensor, cursor)?;
        cursor += 1;

        if cursor == tokens.len() {
            let logits = logits.squeeze(0)?.squeeze(0)?.to_dtype(DType::F32)?;
            next_token = Some(logits_processor.sample(&logits)?);
        }
    }

    let Some(next_token) = next_token else {
        return Ok(InferenceStepResult {
            appended_tokens: Vec::new(),
            emitted_text: String::new(),
            finished: true,
            next_index_pos: cursor,
        });
    };

    let emitted_text = tokenizer.decode(&[next_token], true).unwrap_or_default();
    let total_tokens = tokens.len() + 1;
    let finished = next_token == eos_token_id
        || next_token == eot_token_id
        || next_token == 2
        || total_tokens >= generation.max_tokens;

    Ok(InferenceStepResult {
        appended_tokens: vec![next_token],
        emitted_text,
        finished,
        next_index_pos: cursor,
    })
}

impl InferenceBackend for QuantizedLlamaBackend {
    fn backend_id(&self) -> &'static str {
        "candle.quantized_llama"
    }

    fn family(&self) -> PromptFamily {
        PromptFamily::Llama
    }

    fn generate_step(
        &mut self,
        context_slot_id: Option<ContextSlotId>,
        tokens: &[u32],
        index_pos: usize,
        logits_processor: &mut LogitsProcessor,
        tokenizer: &Tokenizer,
        generation: GenerationConfig,
        device: &Device,
        eos_token_id: u32,
        eot_token_id: u32,
    ) -> Result<InferenceStepResult> {
        generate_local_step(
            context_slot_id,
            tokens,
            index_pos,
            logits_processor,
            tokenizer,
            generation,
            device,
            eos_token_id,
            eot_token_id,
            |input_tensor, position| Ok(self.weights.forward(input_tensor, position)?),
        )
    }

    fn duplicate_boxed(&self) -> Option<Box<dyn ModelBackend>> {
        Some(Box::new(Self {
            weights: self.weights.clone(),
        }))
    }
}

impl ContextSlotPersistence for QuantizedLlamaBackend {
    fn free_context_slot(&mut self, _slot_id: ContextSlotId) -> Result<()> {
        Ok(())
    }
}

pub(crate) struct QuantizedQwen2Backend {
    weights: quantized_qwen2::ModelWeights,
}

impl QuantizedQwen2Backend {
    pub(crate) fn load(path: &str, device: &Device) -> Result<Self> {
        let mut file = std::fs::File::open(path)
            .map_err(|e| E::msg(format!("Failed to open model file: {}", e)))?;
        let content = gguf_file::Content::read(&mut file)?;

        match quantized_qwen2::ModelWeights::from_gguf(content, &mut file, device) {
            Ok(weights) => Ok(Self { weights }),
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
}

impl InferenceBackend for QuantizedQwen2Backend {
    fn backend_id(&self) -> &'static str {
        "candle.quantized_qwen2"
    }

    fn family(&self) -> PromptFamily {
        PromptFamily::Qwen
    }

    fn generate_step(
        &mut self,
        context_slot_id: Option<ContextSlotId>,
        tokens: &[u32],
        index_pos: usize,
        logits_processor: &mut LogitsProcessor,
        tokenizer: &Tokenizer,
        generation: GenerationConfig,
        device: &Device,
        eos_token_id: u32,
        eot_token_id: u32,
    ) -> Result<InferenceStepResult> {
        generate_local_step(
            context_slot_id,
            tokens,
            index_pos,
            logits_processor,
            tokenizer,
            generation,
            device,
            eos_token_id,
            eot_token_id,
            |input_tensor, position| Ok(self.weights.forward(input_tensor, position)?),
        )
    }

    fn duplicate_boxed(&self) -> Option<Box<dyn ModelBackend>> {
        None
    }
}

impl ContextSlotPersistence for QuantizedQwen2Backend {
    fn free_context_slot(&mut self, _slot_id: ContextSlotId) -> Result<()> {
        Ok(())
    }
}