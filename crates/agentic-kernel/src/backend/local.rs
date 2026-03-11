use anyhow::{Error as E, Result};
use candle_core::quantized::gguf_file;
use candle_core::{DType, Device, Tensor};
use candle_transformers::models::quantized_llama;
use candle_transformers::models::quantized_qwen2;

use crate::memory::ContextSlotId;
use crate::prompting::PromptFamily;

use super::{
    ContextSlotPersistence, InferenceBackend, InferenceFinishReason, InferenceStepRequest,
    InferenceStepResult, ModelBackend,
};

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
    request: InferenceStepRequest<'_>,
    mut forward: F,
) -> Result<InferenceStepResult>
where
    F: FnMut(&Tensor, usize) -> Result<Tensor>,
{
    let InferenceStepRequest {
        context_slot_id: _,
        tokens,
        index_pos,
        remaining_generation_budget,
        logits_processor,
        tokenizer,
        generation: _,
        device,
        eos_token_id,
        eot_token_id,
    } = request;
    if remaining_generation_budget == 0 {
        return Ok(InferenceStepResult {
            appended_tokens: Vec::new(),
            emitted_text: String::new(),
            finished: true,
            finish_reason: Some(InferenceFinishReason::TurnBudgetExhausted),
            next_index_pos: index_pos.max(tokens.len()),
        });
    }
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
            finish_reason: Some(InferenceFinishReason::TurnBudgetExhausted),
            next_index_pos: cursor,
        });
    };

    let emitted_text = tokenizer.decode(&[next_token], true).unwrap_or_default();
    let finished_due_to_model =
        next_token == eos_token_id || next_token == eot_token_id || next_token == 2;
    let finished_due_to_budget = !finished_due_to_model && remaining_generation_budget <= 1;
    let finished = finished_due_to_model || finished_due_to_budget;
    let finish_reason = if finished_due_to_model {
        Some(InferenceFinishReason::ModelStop)
    } else if finished_due_to_budget {
        Some(InferenceFinishReason::TurnBudgetExhausted)
    } else {
        None
    };

    Ok(InferenceStepResult {
        appended_tokens: vec![next_token],
        emitted_text,
        finished,
        finish_reason,
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

    fn generate_step(&mut self, request: InferenceStepRequest<'_>) -> Result<InferenceStepResult> {
        generate_local_step(request, |input_tensor, position| {
            Ok(self.weights.forward(input_tensor, position)?)
        })
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
                let msg = e.to_string();
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

    fn generate_step(&mut self, request: InferenceStepRequest<'_>) -> Result<InferenceStepResult> {
        generate_local_step(request, |input_tensor, position| {
            Ok(self.weights.forward(input_tensor, position)?)
        })
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
