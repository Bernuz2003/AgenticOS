use anyhow::{Error as E, Result};
use serde_json::json;
use std::path::Path;

use crate::memory::ContextSlotId;
use crate::prompting::PromptFamily;

use super::http::{HttpEndpoint, HttpJsonResponse, HttpRequestOptions, HttpStreamControl};
use super::remote_adapter::{
    build_completion_request, decode_completion_response, drain_json_objects,
    select_completion_prompt_transport, tool_invocation_end, PromptTransportStrategy,
};
use super::{
    ContextSlotPersistence, InferenceBackend, InferenceFinishReason, InferenceStepRequest,
    InferenceStepResult, ModelBackend,
};

#[derive(Clone)]
pub(crate) struct ExternalLlamaCppBackend {
    endpoint: HttpEndpoint,
    family: PromptFamily,
    timeout_ms: u64,
    chunk_tokens: usize,
}

impl ExternalLlamaCppBackend {
    pub(crate) fn from_env(family: PromptFamily) -> Result<Self> {
        let endpoint = super::external_llamacpp_endpoint().ok_or_else(|| {
            E::msg(
                "External llama.cpp RPC backend requested, but AGENTIC_LLAMACPP_ENDPOINT is not configured.",
            )
        })?;

        Ok(Self {
            endpoint: HttpEndpoint::parse(&endpoint)?,
            family,
            timeout_ms: crate::config::kernel_config().external_llamacpp.timeout_ms,
            chunk_tokens: crate::config::kernel_config()
                .external_llamacpp
                .chunk_tokens
                .max(1),
        })
    }

    pub(crate) fn for_diagnostics(
        endpoint: HttpEndpoint,
        family: PromptFamily,
        timeout_ms: u64,
        chunk_tokens: usize,
    ) -> Self {
        Self {
            endpoint,
            family,
            timeout_ms,
            chunk_tokens,
        }
    }

    pub(crate) fn request_json(
        &self,
        method: &str,
        path: &str,
        payload: Option<&serde_json::Value>,
    ) -> Result<HttpJsonResponse> {
        self.endpoint
            .request_json(method, path, payload, self.timeout_ms)
    }

    pub(crate) fn endpoint_path(&self, path: &str) -> String {
        self.endpoint.joined_path(path)
    }

    fn post_json(&self, path: &str, payload: serde_json::Value) -> Result<serde_json::Value> {
        let response = self.request_json("POST", path, Some(&payload))?;
        if response.status_code != 200 {
            return Err(E::msg(format!(
                "External RPC request failed with status '{}': {}",
                response.status_line,
                response.body.trim()
            )));
        }

        response
            .json
            .ok_or_else(|| E::msg("External RPC returned invalid JSON."))
    }

    fn post_streaming_completion(
        &self,
        payload: serde_json::Value,
        tokenizer: &tokenizers::Tokenizer,
    ) -> Result<StreamingCompletion> {
        let mut accumulator = StreamingCompletionAccumulator::default();
        let response = self.endpoint.request_stream_with_options(
            "POST",
            &self.endpoint.joined_path("/completion"),
            Some(&payload),
            HttpRequestOptions {
                timeout_ms: self.timeout_ms,
                max_request_bytes: usize::MAX,
                max_response_bytes: usize::MAX,
                extra_headers: None,
            },
            |fragment| accumulator.push(fragment, tokenizer),
        )?;
        if response.status_code != 200 {
            return Err(E::msg(format!(
                "External RPC request failed with status '{}': {}",
                response.status_line,
                response.body.trim()
            )));
        }

        accumulator.finish(tokenizer)
    }

    fn slot_filename(path: &Path) -> Result<String> {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                E::msg(format!(
                    "Invalid context-slot path '{}': missing valid filename.",
                    path.display()
                ))
            })?;

        if filename.contains('/') || filename.contains("..") {
            return Err(E::msg(format!(
                "Invalid context-slot filename '{}': must stay within the configured llama.cpp slot-save-path.",
                filename
            )));
        }

        Ok(filename.to_string())
    }

    fn slot_action(
        &self,
        slot_id: ContextSlotId,
        action: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        self.post_json(
            &self
                .endpoint
                .joined_path(&format!("/slots/{}?action={}", slot_id, action)),
            payload,
        )?;
        Ok(())
    }
}

impl InferenceBackend for ExternalLlamaCppBackend {
    fn backend_id(&self) -> &'static str {
        "external-llamacpp"
    }

    fn family(&self) -> PromptFamily {
        self.family
    }

    fn generate_step(&mut self, request: InferenceStepRequest<'_>) -> Result<InferenceStepResult> {
        let InferenceStepRequest {
            context_slot_id,
            tokens,
            rendered_prompt,
            resident_prompt_suffix,
            index_pos,
            remaining_generation_budget,
            tokenizer,
            generation,
            eos_token_id: _,
            eot_token_id: _,
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

        let chunk_tokens = remaining_generation_budget.min(self.chunk_tokens);
        let prompt_transport =
            select_completion_prompt_transport(rendered_prompt, resident_prompt_suffix, false);
        if matches!(
            prompt_transport.strategy,
            PromptTransportStrategy::FullPrompt
        ) && !resident_prompt_suffix.is_empty()
        {
            tracing::debug!(
                slot_id = context_slot_id,
                full_prompt_bytes = rendered_prompt.len(),
                suffix_bytes = resident_prompt_suffix.len(),
                "LLAMACPP: append-only transport unavailable, falling back to full prompt reuse"
            );
        }
        let decoded = self.post_streaming_completion(
            build_completion_request(
                prompt_transport.prompt,
                chunk_tokens,
                context_slot_id,
                generation,
                true,
            ),
            tokenizer,
        )?;

        let finished_due_to_budget =
            !decoded.finished && decoded.appended_tokens.len() >= remaining_generation_budget;

        Ok(InferenceStepResult {
            next_index_pos: index_pos.max(tokens.len()),
            emitted_text: decoded.emitted_text,
            finished: decoded.finished || finished_due_to_budget,
            finish_reason: if decoded.finished {
                Some(InferenceFinishReason::ModelStop)
            } else if finished_due_to_budget {
                Some(InferenceFinishReason::TurnBudgetExhausted)
            } else {
                None
            },
            appended_tokens: decoded.appended_tokens,
        })
    }

    fn duplicate_boxed(&self) -> Option<Box<dyn ModelBackend>> {
        Some(Box::new(self.clone()))
    }
}

impl ContextSlotPersistence for ExternalLlamaCppBackend {
    fn save_context_slot(&self, slot_id: ContextSlotId, path: &Path) -> Result<()> {
        let filename = Self::slot_filename(path)?;
        self.slot_action(slot_id, "save", json!({ "filename": filename }))
    }

    fn load_context_slot(&mut self, slot_id: ContextSlotId, path: &Path) -> Result<()> {
        let filename = Self::slot_filename(path)?;
        self.slot_action(slot_id, "restore", json!({ "filename": filename }))
    }

    fn free_context_slot(&mut self, slot_id: ContextSlotId) -> Result<()> {
        self.slot_action(slot_id, "erase", json!({}))
    }
}

#[derive(Debug, Default)]
struct StreamingCompletionAccumulator {
    body_buffer: Vec<u8>,
    emitted_text: String,
    finished: bool,
    stopped_on_tool_marker: bool,
}

impl StreamingCompletionAccumulator {
    fn push(
        &mut self,
        fragment: &[u8],
        tokenizer: &tokenizers::Tokenizer,
    ) -> Result<HttpStreamControl> {
        self.body_buffer.extend_from_slice(fragment);
        for object in drain_json_objects(&mut self.body_buffer)? {
            let decoded = decode_completion_response(object, tokenizer)?;
            self.emitted_text.push_str(&decoded.emitted_text);
            self.finished = decoded.finished;

            if let Some(end) = tool_invocation_end(&self.emitted_text) {
                self.emitted_text.truncate(end);
                self.stopped_on_tool_marker = true;
                self.finished = false;
                return Ok(HttpStreamControl::Stop);
            }

            if self.finished {
                return Ok(HttpStreamControl::Stop);
            }
        }

        Ok(HttpStreamControl::Continue)
    }

    fn finish(self, tokenizer: &tokenizers::Tokenizer) -> Result<StreamingCompletion> {
        let appended_tokens = if self.emitted_text.is_empty() {
            Vec::new()
        } else {
            tokenizer
                .encode(self.emitted_text.as_str(), false)
                .map_err(|e| {
                    E::msg(format!(
                        "Failed to tokenize streamed RPC completion chunk: {}",
                        e
                    ))
                })?
                .get_ids()
                .to_vec()
        };

        Ok(StreamingCompletion {
            emitted_text: self.emitted_text,
            appended_tokens,
            finished: self.finished && !self.stopped_on_tool_marker,
        })
    }
}

struct StreamingCompletion {
    emitted_text: String,
    appended_tokens: Vec<u32>,
    finished: bool,
}
