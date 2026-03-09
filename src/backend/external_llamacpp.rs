use anyhow::{Error as E, Result};
use candle_core::Device;
use candle_transformers::generation::LogitsProcessor;
use serde_json::json;
use std::path::Path;
use tokenizers::Tokenizer;

use crate::memory::ContextSlotId;
use crate::prompting::{GenerationConfig, PromptFamily};

use super::http::{HttpEndpoint, HttpJsonResponse};
use super::remote_adapter::{build_completion_request, decode_completion_response};
use super::{ContextSlotPersistence, InferenceBackend, InferenceStepResult, ModelBackend};

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

    fn slot_filename(path: &Path) -> Result<String> {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| E::msg(format!("Invalid context-slot path '{}': missing valid filename.", path.display())))?;

        if filename.contains('/') || filename.contains("..") {
            return Err(E::msg(format!(
                "Invalid context-slot filename '{}': must stay within the configured llama.cpp slot-save-path.",
                filename
            )));
        }

        Ok(filename.to_string())
    }

    fn slot_action(&self, slot_id: ContextSlotId, action: &str, payload: serde_json::Value) -> Result<()> {
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

    fn generate_step(
        &mut self,
        context_slot_id: Option<ContextSlotId>,
        tokens: &[u32],
        index_pos: usize,
        _logits_processor: &mut LogitsProcessor,
        tokenizer: &Tokenizer,
        generation: GenerationConfig,
        _device: &Device,
        _eos_token_id: u32,
        _eot_token_id: u32,
    ) -> Result<InferenceStepResult> {
        if tokens.len() >= generation.max_tokens {
            return Ok(InferenceStepResult {
                appended_tokens: Vec::new(),
                emitted_text: String::new(),
                finished: true,
                next_index_pos: index_pos.max(tokens.len()),
            });
        }

        let chunk_tokens = generation
            .max_tokens
            .saturating_sub(tokens.len())
            .min(self.chunk_tokens);
        let prompt = tokenizer
            .decode(tokens, false)
            .map_err(|e| E::msg(format!("Failed to decode prompt tokens for RPC backend: {}", e)))?;
        let raw = self.post_json(
            &self.endpoint.joined_path("/completion"),
            build_completion_request(&prompt, chunk_tokens, context_slot_id, generation),
        )?;
        let decoded = decode_completion_response(raw, tokenizer)?;

        Ok(InferenceStepResult {
            next_index_pos: index_pos.max(tokens.len()),
            emitted_text: decoded.emitted_text,
            finished: decoded.finished || tokens.len() + decoded.appended_tokens.len() >= generation.max_tokens,
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
