use agentic_control_models::BackendTelemetryView;
use anyhow::{Error as E, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokenizers::Tokenizer;

use crate::accounting::{AccountingEventStatus, BackendAccountingEvent};
use crate::config::RemoteProviderRuntimeConfig;
use crate::model_catalog::RemoteModelEntry;
use crate::prompting::PromptFamily;

use crate::backend::remote_adapter::{drain_json_objects, tool_invocation_end};
use crate::backend::{
    BackendCapabilities, InferenceBackend, InferenceFinishReason, InferenceStepRequest,
    InferenceStepResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteOpenAITransport {
    ResponsesApi,
    ChatCompletions,
}

#[derive(Debug, Clone, Copy)]
struct RemoteOpenAIProviderProfile {
    backend_id: &'static str,
    display_name: &'static str,
    config_section: &'static str,
    endpoint_env: &'static str,
    api_key_env: &'static str,
    fallback_api_key_env: Option<&'static str>,
    transport: RemoteOpenAITransport,
}

const OPENAI_RESPONSES_PROFILE: RemoteOpenAIProviderProfile = RemoteOpenAIProviderProfile {
    backend_id: "openai-responses",
    display_name: "OpenAI Responses",
    config_section: "openai_responses",
    endpoint_env: "AGENTIC_OPENAI_ENDPOINT",
    api_key_env: "AGENTIC_OPENAI_API_KEY",
    fallback_api_key_env: Some("OPENAI_API_KEY"),
    transport: RemoteOpenAITransport::ResponsesApi,
};

const GROQ_RESPONSES_PROFILE: RemoteOpenAIProviderProfile = RemoteOpenAIProviderProfile {
    backend_id: "groq-responses",
    display_name: "Groq Responses",
    config_section: "groq_responses",
    endpoint_env: "AGENTIC_GROQ_ENDPOINT",
    api_key_env: "AGENTIC_GROQ_API_KEY",
    fallback_api_key_env: Some("GROQ_API_KEY"),
    transport: RemoteOpenAITransport::ResponsesApi,
};

const OPENROUTER_PROFILE: RemoteOpenAIProviderProfile = RemoteOpenAIProviderProfile {
    backend_id: "openrouter",
    display_name: "OpenRouter",
    config_section: "openrouter",
    endpoint_env: "AGENTIC_OPENROUTER_ENDPOINT",
    api_key_env: "AGENTIC_OPENROUTER_API_KEY",
    fallback_api_key_env: Some("OPENROUTER_API_KEY"),
    transport: RemoteOpenAITransport::ChatCompletions,
};

fn provider_profile(backend_id: &str) -> Option<&'static RemoteOpenAIProviderProfile> {
    match backend_id {
        "openai-responses" => Some(&OPENAI_RESPONSES_PROFILE),
        "groq-responses" => Some(&GROQ_RESPONSES_PROFILE),
        "openrouter" => Some(&OPENROUTER_PROFILE),
        _ => None,
    }
}

#[derive(Clone)]
pub(crate) struct RemoteOpenAICompatibleBackend {
    profile: &'static RemoteOpenAIProviderProfile,
    family: PromptFamily,
    endpoint: String,
    api_key: String,
    model: String,
    model_spec: RemoteModelEntry,
    timeout_ms: u64,
    max_request_bytes: usize,
    max_response_bytes: usize,
    stream: bool,
    input_price_usd_per_mtok: f64,
    output_price_usd_per_mtok: f64,
    http_referer: Option<String>,
    app_title: Option<String>,
    last_accounting_event: Option<BackendAccountingEvent>,
}

#[derive(Debug, Default, Clone)]
struct RemoteOpenAICompatibleTelemetry {
    requests_total: u64,
    stream_requests_total: u64,
    input_tokens_total: u64,
    output_tokens_total: u64,
    estimated_cost_usd: f64,
    rate_limit_errors: u64,
    auth_errors: u64,
    transport_errors: u64,
    last_model: Option<String>,
    last_error: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct UsageSnapshot {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
struct DecodedResponse {
    emitted_text: String,
    finished: bool,
    usage: UsageSnapshot,
}

#[derive(Debug, Clone)]
struct RequestFailure {
    status: AccountingEventStatus,
    error_code: Option<String>,
    error_message: String,
}

impl RemoteOpenAICompatibleBackend {
    pub(crate) fn from_env(family: PromptFamily, backend_id: &str, model_id: &str) -> Result<Self> {
        let profile = provider_profile(backend_id)
            .ok_or_else(|| E::msg(format!("Unknown remote backend profile '{}'.", backend_id)))?;
        let config = super::runtime_config(backend_id).ok_or_else(|| {
            E::msg(format!(
                "Missing runtime config for backend '{}'.",
                backend_id
            ))
        })?;
        Self::from_config(
            profile,
            family,
            RemoteModelEntry {
                id: model_id.trim().to_string(),
                label: model_id.trim().to_string(),
                context_window_tokens: None,
                max_output_tokens: None,
                supports_structured_output: true,
                input_price_usd_per_mtok: None,
                output_price_usd_per_mtok: None,
            },
            config,
        )
    }

    pub(crate) fn from_runtime(
        family: PromptFamily,
        backend_id: &str,
        model_spec: RemoteModelEntry,
        config: RemoteProviderRuntimeConfig,
    ) -> Result<Self> {
        let profile = provider_profile(backend_id)
            .ok_or_else(|| E::msg(format!("Unknown remote backend profile '{}'.", backend_id)))?;
        Self::from_config(profile, family, model_spec, config)
    }

    fn from_config(
        profile: &'static RemoteOpenAIProviderProfile,
        family: PromptFamily,
        model_spec: RemoteModelEntry,
        config: RemoteProviderRuntimeConfig,
    ) -> Result<Self> {
        let endpoint = config.endpoint.trim().trim_end_matches('/').to_string();
        if endpoint.is_empty() {
            return Err(E::msg(format!(
                "{} endpoint is not configured. Set [{}].endpoint or {}.",
                profile.display_name, profile.config_section, profile.endpoint_env
            )));
        }

        let api_key = config.api_key.trim().to_string();
        if api_key.is_empty() {
            let env_hint = profile
                .fallback_api_key_env
                .map(|fallback| {
                    format!(
                        "{}, {} or {}",
                        profile.api_key_env, fallback, "the config section"
                    )
                })
                .unwrap_or_else(|| format!("{} or the config section", profile.api_key_env));
            return Err(E::msg(format!(
                "{} API key is not configured. Set [{}].api_key, {}.",
                profile.display_name, profile.config_section, env_hint
            )));
        }

        let model = (!model_spec.id.trim().is_empty())
            .then(|| model_spec.id.trim().to_string())
            .or_else(|| {
                let configured = config.default_model.trim();
                (!configured.is_empty()).then(|| configured.to_string())
            })
            .ok_or_else(|| {
                E::msg(format!(
                    "{} backend requires a model reference. Use LOAD cloud:{}:<model> or configure [{}].default_model.",
                    profile.display_name, profile.backend_id, profile.config_section
                ))
            })?;
        ensure_telemetry_entry(profile.backend_id, &model);

        Ok(Self {
            profile,
            family,
            endpoint,
            api_key,
            model,
            model_spec: model_spec.clone(),
            timeout_ms: config.timeout_ms.max(1),
            max_request_bytes: config.max_request_bytes.max(1024),
            max_response_bytes: config.max_response_bytes.max(1024),
            stream: config.stream,
            input_price_usd_per_mtok: model_spec
                .input_price_usd_per_mtok
                .unwrap_or(config.input_price_usd_per_mtok)
                .max(0.0),
            output_price_usd_per_mtok: model_spec
                .output_price_usd_per_mtok
                .unwrap_or(config.output_price_usd_per_mtok)
                .max(0.0),
            http_referer: trimmed_option(&config.http_referer),
            app_title: trimmed_option(&config.app_title),
            last_accounting_event: None,
        })
    }

    fn request_url(&self) -> String {
        let path = match self.profile.transport {
            RemoteOpenAITransport::ResponsesApi => "responses",
            RemoteOpenAITransport::ChatCompletions => "chat/completions",
        };
        format!("{}/{}", self.endpoint, path)
    }

    fn request_payload(
        &self,
        rendered_prompt: &str,
        remaining_generation_budget: usize,
        generation: crate::prompting::GenerationConfig,
    ) -> serde_json::Value {
        let effective_max_output_tokens = self
            .model_spec
            .max_output_tokens
            .map(|limit| limit.min(remaining_generation_budget))
            .unwrap_or(remaining_generation_budget);
        match self.profile.transport {
            RemoteOpenAITransport::ResponsesApi => json!({
                "model": self.model,
                "input": rendered_prompt,
                "temperature": generation.temperature,
                "top_p": generation.top_p,
                "max_output_tokens": effective_max_output_tokens,
                "stream": self.stream,
            }),
            RemoteOpenAITransport::ChatCompletions => json!({
                "model": self.model,
                "prompt": rendered_prompt,
                "temperature": generation.temperature,
                "top_p": generation.top_p,
                "max_tokens": effective_max_output_tokens,
                "stream": self.stream,
            }),
        }
    }

    fn send_request(
        &mut self,
        payload: &serde_json::Value,
        tokenizer: &Tokenizer,
        estimated_input_tokens: u64,
    ) -> Result<DecodedResponse> {
        self.last_accounting_event = None;
        let request_body = payload.to_string();
        if request_body.len() > self.max_request_bytes {
            return Err(E::msg(format!(
                "{} request exceeded limit ({} > {} bytes).",
                self.profile.display_name,
                request_body.len(),
                self.max_request_bytes
            )));
        }

        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_millis(self.timeout_ms))
            .timeout_read(Duration::from_millis(self.timeout_ms))
            .timeout_write(Duration::from_millis(self.timeout_ms))
            .build();

        record_attempt(self.profile.backend_id, self.stream, &self.model);
        let mut request = agent
            .post(&self.request_url())
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .set(
                "Accept",
                if self.stream {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            );

        if let Some(http_referer) = self.http_referer.as_deref() {
            request = request.set("HTTP-Referer", http_referer);
        }
        if let Some(app_title) = self.app_title.as_deref() {
            request = request.set("X-OpenRouter-Title", app_title);
        }

        let response = request.send_string(&request_body).map_err(|err| {
            let failure = map_ureq_error(self.profile, err, &self.model);
            self.record_failure_event(&failure, estimated_input_tokens);
            E::msg(format_failure_message(self.profile, &failure))
        })?;

        if self.stream {
            decode_streaming_response(
                self.profile,
                response.into_reader(),
                self.max_response_bytes,
                tokenizer,
            )
            .map_err(|err| {
                let failure = RequestFailure {
                    status: AccountingEventStatus::HttpError,
                    error_code: None,
                    error_message: err.to_string(),
                };
                record_transport_error(
                    self.profile.backend_id,
                    &self.model,
                    &failure.error_message,
                );
                self.record_failure_event(&failure, estimated_input_tokens);
                E::msg(failure.error_message)
            })
        } else {
            let body = response.into_string().map_err(|err| {
                let failure = RequestFailure {
                    status: AccountingEventStatus::TransportError,
                    error_code: None,
                    error_message: format!(
                        "Failed to read {} payload for model '{}': {}",
                        self.profile.display_name, self.model, err
                    ),
                };
                record_transport_error(
                    self.profile.backend_id,
                    &self.model,
                    &failure.error_message,
                );
                self.record_failure_event(&failure, estimated_input_tokens);
                E::msg(failure.error_message)
            })?;
            if body.len() > self.max_response_bytes {
                let failure = RequestFailure {
                    status: AccountingEventStatus::TransportError,
                    error_code: None,
                    error_message: format!(
                        "{} payload exceeded limit ({} > {} bytes).",
                        self.profile.display_name,
                        body.len(),
                        self.max_response_bytes
                    ),
                };
                record_transport_error(
                    self.profile.backend_id,
                    &self.model,
                    &failure.error_message,
                );
                self.record_failure_event(&failure, estimated_input_tokens);
                return Err(E::msg(failure.error_message));
            }
            decode_non_streaming_response(self.profile, &body, tokenizer).map_err(|err| {
                let failure = RequestFailure {
                    status: AccountingEventStatus::HttpError,
                    error_code: None,
                    error_message: err.to_string(),
                };
                record_transport_error(
                    self.profile.backend_id,
                    &self.model,
                    &failure.error_message,
                );
                self.record_failure_event(&failure, estimated_input_tokens);
                E::msg(failure.error_message)
            })
        }
    }

    fn record_success_event(
        &mut self,
        input_tokens: u64,
        output_tokens: u64,
        provider_reported_cost_usd: Option<f64>,
    ) {
        self.last_accounting_event = Some(BackendAccountingEvent {
            backend_id: self.profile.backend_id.to_string(),
            model_id: Some(self.model.clone()),
            request_count: 1,
            stream: self.stream,
            input_tokens,
            output_tokens,
            estimated_cost_usd: resolve_cost_usd(
                input_tokens,
                output_tokens,
                self.input_price_usd_per_mtok,
                self.output_price_usd_per_mtok,
                provider_reported_cost_usd,
            ),
            status: AccountingEventStatus::Success,
            error_code: None,
            error_message: None,
        });
    }

    fn record_failure_event(&mut self, failure: &RequestFailure, estimated_input_tokens: u64) {
        self.last_accounting_event = Some(BackendAccountingEvent {
            backend_id: self.profile.backend_id.to_string(),
            model_id: Some(self.model.clone()),
            request_count: 1,
            stream: self.stream,
            input_tokens: estimated_input_tokens,
            output_tokens: 0,
            estimated_cost_usd: 0.0,
            status: failure.status,
            error_code: failure.error_code.clone(),
            error_message: Some(failure.error_message.clone()),
        });
    }

    fn duplicate_for_process(&self) -> Self {
        let mut cloned = self.clone();
        cloned.last_accounting_event = None;
        cloned
    }
}

fn resolve_cost_usd(
    input_tokens: u64,
    output_tokens: u64,
    input_price_usd_per_mtok: f64,
    output_price_usd_per_mtok: f64,
    provider_reported_cost_usd: Option<f64>,
) -> f64 {
    provider_reported_cost_usd.unwrap_or_else(|| {
        (input_tokens as f64 / 1_000_000.0) * input_price_usd_per_mtok
            + (output_tokens as f64 / 1_000_000.0) * output_price_usd_per_mtok
    })
}

fn format_failure_message(
    profile: &RemoteOpenAIProviderProfile,
    failure: &RequestFailure,
) -> String {
    match failure.status {
        AccountingEventStatus::RateLimitError | AccountingEventStatus::AuthError => {
            format!(
                "{} returned {}: {}",
                profile.display_name,
                failure.error_code.as_deref().unwrap_or("error"),
                failure.error_message
            )
        }
        AccountingEventStatus::TransportError => {
            format!(
                "{} transport error: {}",
                profile.display_name, failure.error_message
            )
        }
        AccountingEventStatus::HttpError | AccountingEventStatus::Success => {
            failure.error_message.clone()
        }
    }
}

impl InferenceBackend for RemoteOpenAICompatibleBackend {
    fn backend_id(&self) -> &'static str {
        self.profile.backend_id
    }

    fn family(&self) -> PromptFamily {
        self.family
    }

    fn generate_step(&mut self, request: InferenceStepRequest<'_>) -> Result<InferenceStepResult> {
        let InferenceStepRequest {
            tokens,
            rendered_prompt,
            index_pos,
            remaining_generation_budget,
            tokenizer,
            generation,
            ..
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

        let payload =
            self.request_payload(rendered_prompt, remaining_generation_budget, generation);
        let estimated_input_tokens = estimate_token_count(tokenizer, rendered_prompt) as u64;
        let decoded = self.send_request(&payload, tokenizer, estimated_input_tokens)?;
        let appended_tokens = if decoded.emitted_text.is_empty() {
            Vec::new()
        } else {
            tokenizer
                .encode(decoded.emitted_text.as_str(), false)
                .map_err(|err| {
                    E::msg(format!(
                        "Failed to tokenize {} output for model '{}': {}",
                        self.profile.display_name, self.model, err
                    ))
                })?
                .get_ids()
                .to_vec()
        };
        let input_tokens = decoded.usage.input_tokens.unwrap_or(estimated_input_tokens);
        let output_tokens = decoded
            .usage
            .output_tokens
            .unwrap_or(appended_tokens.len() as u64);
        record_success(
            self.profile.backend_id,
            &self.model,
            input_tokens,
            output_tokens,
            self.input_price_usd_per_mtok,
            self.output_price_usd_per_mtok,
            decoded.usage.estimated_cost_usd,
        );
        self.record_success_event(
            input_tokens,
            output_tokens,
            decoded.usage.estimated_cost_usd,
        );
        let finished_due_to_budget =
            !decoded.finished && appended_tokens.len() >= remaining_generation_budget;

        Ok(InferenceStepResult {
            appended_tokens,
            emitted_text: decoded.emitted_text,
            finished: decoded.finished || finished_due_to_budget,
            finish_reason: if decoded.finished {
                Some(InferenceFinishReason::ModelStop)
            } else if finished_due_to_budget {
                Some(InferenceFinishReason::TurnBudgetExhausted)
            } else {
                None
            },
            next_index_pos: index_pos.max(tokens.len()),
        })
    }

    fn duplicate_boxed(&self) -> Option<Box<dyn crate::backend::ModelBackend>> {
        Some(Box::new(self.duplicate_for_process()))
    }

    fn take_last_accounting_event(&mut self) -> Option<BackendAccountingEvent> {
        self.last_accounting_event.take()
    }

    fn runtime_capabilities(&self) -> Option<BackendCapabilities> {
        Some(BackendCapabilities {
            structured_output: self.model_spec.supports_structured_output,
            ..super::CAP_REMOTE_OPENAI_COMPATIBLE
        })
    }
}

impl crate::backend::ContextSlotPersistence for RemoteOpenAICompatibleBackend {}

pub(crate) fn telemetry_snapshot(backend_id: &str) -> Option<BackendTelemetryView> {
    telemetry_cell()
        .lock()
        .expect("lock remote openai telemetry")
        .get(backend_id)
        .cloned()
        .map(|telemetry| BackendTelemetryView {
            requests_total: telemetry.requests_total,
            stream_requests_total: telemetry.stream_requests_total,
            input_tokens_total: telemetry.input_tokens_total,
            output_tokens_total: telemetry.output_tokens_total,
            estimated_cost_usd: telemetry.estimated_cost_usd,
            rate_limit_errors: telemetry.rate_limit_errors,
            auth_errors: telemetry.auth_errors,
            transport_errors: telemetry.transport_errors,
            last_model: telemetry.last_model,
            last_error: telemetry.last_error,
        })
}

#[cfg(test)]
pub(crate) fn reset_telemetry(backend_id: Option<&str>) {
    let mut telemetry = telemetry_cell()
        .lock()
        .expect("lock remote openai telemetry");
    if let Some(backend_id) = backend_id {
        telemetry.remove(backend_id);
    } else {
        telemetry.clear();
    }
}

fn trimmed_option(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn map_ureq_error(
    profile: &RemoteOpenAIProviderProfile,
    err: ureq::Error,
    model: &str,
) -> RequestFailure {
    match err {
        ureq::Error::Status(code, response) => {
            let body = response.into_string().unwrap_or_default();
            record_http_error(profile.backend_id, code, model, &body);
            RequestFailure {
                status: if code == 429 {
                    AccountingEventStatus::RateLimitError
                } else if matches!(code, 401 | 403) {
                    AccountingEventStatus::AuthError
                } else {
                    AccountingEventStatus::HttpError
                },
                error_code: Some(code.to_string()),
                error_message: body,
            }
        }
        ureq::Error::Transport(transport) => {
            record_transport_error(profile.backend_id, model, &transport.to_string());
            RequestFailure {
                status: AccountingEventStatus::TransportError,
                error_code: None,
                error_message: transport.to_string(),
            }
        }
    }
}

fn estimate_token_count(tokenizer: &Tokenizer, text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }

    tokenizer
        .encode(text, false)
        .map(|encoding| encoding.len())
        .unwrap_or_else(|_| text.split_whitespace().count().max(1))
}

fn telemetry_cell() -> &'static Mutex<BTreeMap<String, RemoteOpenAICompatibleTelemetry>> {
    static TELEMETRY: OnceLock<Mutex<BTreeMap<String, RemoteOpenAICompatibleTelemetry>>> =
        OnceLock::new();
    TELEMETRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn record_attempt(backend_id: &str, stream: bool, model: &str) {
    let mut telemetry = telemetry_cell()
        .lock()
        .expect("lock remote openai telemetry");
    let entry = telemetry.entry(backend_id.to_string()).or_default();
    entry.requests_total += 1;
    if stream {
        entry.stream_requests_total += 1;
    }
    entry.last_model = Some(model.to_string());
}

fn ensure_telemetry_entry(backend_id: &str, model: &str) {
    let mut telemetry = telemetry_cell()
        .lock()
        .expect("lock remote openai telemetry");
    let entry = telemetry.entry(backend_id.to_string()).or_default();
    if entry.last_model.is_none() {
        entry.last_model = Some(model.to_string());
    }
}

fn record_success(
    backend_id: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    input_price_usd_per_mtok: f64,
    output_price_usd_per_mtok: f64,
    provider_reported_cost_usd: Option<f64>,
) {
    let mut telemetry = telemetry_cell()
        .lock()
        .expect("lock remote openai telemetry");
    let entry = telemetry.entry(backend_id.to_string()).or_default();
    entry.input_tokens_total += input_tokens;
    entry.output_tokens_total += output_tokens;
    entry.estimated_cost_usd += provider_reported_cost_usd.unwrap_or_else(|| {
        (input_tokens as f64 / 1_000_000.0) * input_price_usd_per_mtok
            + (output_tokens as f64 / 1_000_000.0) * output_price_usd_per_mtok
    });
    entry.last_model = Some(model.to_string());
    entry.last_error = None;
}

fn record_http_error(backend_id: &str, status_code: u16, model: &str, message: &str) {
    let mut telemetry = telemetry_cell()
        .lock()
        .expect("lock remote openai telemetry");
    let entry = telemetry.entry(backend_id.to_string()).or_default();
    if status_code == 429 {
        entry.rate_limit_errors += 1;
    } else if matches!(status_code, 401 | 403) {
        entry.auth_errors += 1;
    } else {
        entry.transport_errors += 1;
    }
    entry.last_model = Some(model.to_string());
    entry.last_error = Some(message.to_string());
}

fn record_transport_error(backend_id: &str, model: &str, message: &str) {
    let mut telemetry = telemetry_cell()
        .lock()
        .expect("lock remote openai telemetry");
    let entry = telemetry.entry(backend_id.to_string()).or_default();
    entry.transport_errors += 1;
    entry.last_model = Some(model.to_string());
    entry.last_error = Some(message.to_string());
}

fn decode_streaming_response<R: Read>(
    profile: &RemoteOpenAIProviderProfile,
    mut reader: R,
    max_response_bytes: usize,
    tokenizer: &Tokenizer,
) -> Result<DecodedResponse> {
    let mut raw_bytes = 0usize;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];

    loop {
        let read = reader.read(&mut chunk).map_err(|err| {
            E::msg(format!(
                "Failed to read {} stream: {}",
                profile.display_name, err
            ))
        })?;
        if read == 0 {
            break;
        }
        raw_bytes += read;
        if raw_bytes > max_response_bytes {
            return Err(E::msg(format!(
                "{} stream exceeded limit ({} > {} bytes).",
                profile.display_name, raw_bytes, max_response_bytes
            )));
        }
        buffer.extend_from_slice(&chunk[..read]);
    }

    match profile.transport {
        RemoteOpenAITransport::ResponsesApi => {
            decode_responses_stream(profile, &mut buffer, tokenizer)
        }
        RemoteOpenAITransport::ChatCompletions => {
            decode_chat_completions_stream(profile, &mut buffer, tokenizer)
        }
    }
}

fn decode_responses_stream(
    profile: &RemoteOpenAIProviderProfile,
    buffer: &mut Vec<u8>,
    _tokenizer: &Tokenizer,
) -> Result<DecodedResponse> {
    let mut emitted_text = String::new();
    let mut finished = false;
    let mut incomplete = false;

    for event in drain_json_objects(buffer)? {
        let event_type = event
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        match event_type {
            "response.output_text.delta" => {
                if let Some(delta) = event.get("delta").and_then(|value| value.as_str()) {
                    emitted_text.push_str(delta);
                }
            }
            "response.output_text.done" => {
                if let Some(text) = event.get("text").and_then(|value| value.as_str()) {
                    if emitted_text.is_empty() {
                        emitted_text.push_str(text);
                    }
                }
            }
            "response.completed" => finished = true,
            "response.incomplete" => incomplete = true,
            "error" | "response.failed" => {
                return Err(E::msg(format!(
                    "{} stream failed: {}",
                    profile.display_name,
                    extract_error_message(&event)
                )));
            }
            _ => {}
        }

        if let Some(end) = tool_invocation_end(&emitted_text) {
            emitted_text.truncate(end);
            return Ok(DecodedResponse {
                emitted_text,
                finished: false,
                usage: UsageSnapshot::default(),
            });
        }
    }

    if !finished && !incomplete && !emitted_text.is_empty() {
        finished = true;
    }

    Ok(DecodedResponse {
        emitted_text,
        finished: finished && !incomplete,
        usage: UsageSnapshot::default(),
    })
}

fn decode_chat_completions_stream(
    profile: &RemoteOpenAIProviderProfile,
    buffer: &mut Vec<u8>,
    _tokenizer: &Tokenizer,
) -> Result<DecodedResponse> {
    let mut emitted_text = String::new();
    let mut usage = UsageSnapshot::default();
    let mut finished = false;
    let mut hit_length_limit = false;

    for event in drain_json_objects(buffer)? {
        if event.get("error").is_some() {
            return Err(E::msg(format!(
                "{} request failed: {}",
                profile.display_name,
                extract_error_message(&event)
            )));
        }

        if let Some(delta) = extract_chat_completions_text(&event) {
            emitted_text.push_str(&delta);
        }
        usage = merge_usage(usage, extract_usage_snapshot(&event));

        if let Some(finish_reason) = extract_chat_completions_finish_reason(&event) {
            match finish_reason.as_str() {
                "length" => hit_length_limit = true,
                "stop" | "tool_calls" => finished = true,
                _ => {}
            }
        }

        if let Some(end) = tool_invocation_end(&emitted_text) {
            emitted_text.truncate(end);
            return Ok(DecodedResponse {
                emitted_text,
                finished: false,
                usage,
            });
        }
    }

    if !finished && !hit_length_limit && !emitted_text.is_empty() {
        finished = true;
    }

    Ok(DecodedResponse {
        emitted_text,
        finished: finished && !hit_length_limit,
        usage,
    })
}

fn decode_non_streaming_response(
    profile: &RemoteOpenAIProviderProfile,
    body: &str,
    _tokenizer: &Tokenizer,
) -> Result<DecodedResponse> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|err| {
        E::msg(format!(
            "Malformed {} payload returned by remote API: {}",
            profile.display_name, err
        ))
    })?;

    if json.get("error").is_some() {
        return Err(E::msg(format!(
            "{} request failed: {}",
            profile.display_name,
            extract_error_message(&json)
        )));
    }

    match profile.transport {
        RemoteOpenAITransport::ResponsesApi => Ok(DecodedResponse {
            emitted_text: extract_responses_output_text(&json),
            finished: !matches!(
                json.get("status").and_then(|value| value.as_str()),
                Some("incomplete")
            ),
            usage: UsageSnapshot::default(),
        }),
        RemoteOpenAITransport::ChatCompletions => {
            let finish_reason = extract_chat_completions_finish_reason(&json);
            Ok(DecodedResponse {
                emitted_text: extract_chat_completions_text(&json).unwrap_or_default(),
                finished: !matches!(finish_reason.as_deref(), Some("length")),
                usage: extract_usage_snapshot(&json),
            })
        }
    }
}

fn extract_responses_output_text(json: &serde_json::Value) -> String {
    if let Some(text) = json.get("output_text").and_then(|value| value.as_str()) {
        return text.to_string();
    }

    json.get("output")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("content").and_then(|value| value.as_array()))
        .flatten()
        .filter_map(|content| {
            content
                .get("text")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .collect::<Vec<_>>()
        .join("")
}

fn extract_chat_completions_text(json: &serde_json::Value) -> Option<String> {
    let choice = json
        .get("choices")
        .and_then(|value| value.as_array())
        .and_then(|choices| choices.first())?;

    choice
        .get("message")
        .and_then(|value| value.get("content"))
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            choice
                .get("delta")
                .and_then(|value| value.get("content"))
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .or_else(|| {
            choice
                .get("text")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
}

fn extract_chat_completions_finish_reason(json: &serde_json::Value) -> Option<String> {
    json.get("choices")
        .and_then(|value| value.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
}

fn extract_usage_snapshot(json: &serde_json::Value) -> UsageSnapshot {
    let usage = json.get("usage");
    UsageSnapshot {
        input_tokens: usage
            .and_then(|value| value.get("prompt_tokens"))
            .and_then(|value| value.as_u64()),
        output_tokens: usage
            .and_then(|value| value.get("completion_tokens"))
            .and_then(|value| value.as_u64()),
        estimated_cost_usd: usage
            .and_then(|value| value.get("cost"))
            .and_then(|value| value.as_f64())
            .or_else(|| json.get("cost").and_then(|value| value.as_f64())),
    }
}

fn merge_usage(current: UsageSnapshot, next: UsageSnapshot) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens: next.input_tokens.or(current.input_tokens),
        output_tokens: next.output_tokens.or(current.output_tokens),
        estimated_cost_usd: next.estimated_cost_usd.or(current.estimated_cost_usd),
    }
}

fn extract_error_message(json: &serde_json::Value) -> String {
    json.get("error")
        .and_then(|value| value.get("message").or(Some(value)))
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            json.get("message")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| json.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        decode_non_streaming_response, decode_streaming_response, extract_chat_completions_text,
        extract_responses_output_text, provider_profile, record_http_error, record_transport_error,
        reset_telemetry, telemetry_snapshot,
    };
    use std::io::Cursor;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use tokenizers::models::wordlevel::WordLevel;
    use tokenizers::pre_tokenizers::whitespace::Whitespace;
    use tokenizers::Tokenizer;

    fn test_tokenizer() -> Tokenizer {
        let vocab = [
            ("<unk>".to_string(), 0),
            ("hello".to_string(), 1),
            ("world".to_string(), 2),
        ]
        .into_iter()
        .collect();

        let model = WordLevel::builder()
            .vocab(vocab)
            .unk_token("<unk>".to_string())
            .build()
            .expect("build tokenizer");

        let mut tokenizer = Tokenizer::new(model);
        tokenizer.with_pre_tokenizer(Some(Whitespace));
        tokenizer
    }

    fn telemetry_test_guard() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("lock telemetry test guard")
    }

    #[test]
    fn non_streaming_responses_output_text_is_extracted_from_output_blocks() {
        let payload = r#"{
            "status":"completed",
            "output":[
                {"content":[{"type":"output_text","text":"hello "},{"type":"output_text","text":"world"}]}
            ]
        }"#;

        let decoded = decode_non_streaming_response(
            provider_profile("openai-responses").expect("openai profile"),
            payload,
            &test_tokenizer(),
        )
        .expect("decode payload");

        assert_eq!(decoded.emitted_text, "hello world");
        assert!(decoded.finished);
    }

    #[test]
    fn extract_responses_output_text_prefers_top_level_field_when_available() {
        let payload = serde_json::json!({
            "output_text": "hello world",
            "output": [
                {"content": [{"text": "ignored"}]}
            ]
        });

        assert_eq!(extract_responses_output_text(&payload), "hello world");
    }

    #[test]
    fn streaming_responses_decoder_truncates_at_tool_invocation_boundary() {
        let stream = Cursor::new(
            br#"data: {"type":"response.output_text.delta","delta":"TOOL:calc {\"expression\":"}

data: {"type":"response.output_text.delta","delta":"\"1+1\"}"}

data: {"type":"response.output_text.delta","delta":"\nI should not continue"}

"#,
        );

        let decoded = decode_streaming_response(
            provider_profile("openai-responses").expect("openai profile"),
            stream,
            4096,
            &test_tokenizer(),
        )
        .expect("decode stream");

        assert_eq!(decoded.emitted_text, "TOOL:calc {\"expression\":\"1+1\"}");
        assert!(!decoded.finished);
    }

    #[test]
    fn non_streaming_chat_completions_extracts_prompt_text_and_usage() {
        let payload = r#"{
            "choices":[{"text":"hello world","finish_reason":"stop"}],
            "usage":{"prompt_tokens":11,"completion_tokens":2,"cost":0.00031}
        }"#;

        let decoded = decode_non_streaming_response(
            provider_profile("openrouter").expect("openrouter profile"),
            payload,
            &test_tokenizer(),
        )
        .expect("decode payload");

        assert_eq!(decoded.emitted_text, "hello world");
        assert_eq!(decoded.usage.input_tokens, Some(11));
        assert_eq!(decoded.usage.output_tokens, Some(2));
        assert_eq!(decoded.usage.estimated_cost_usd, Some(0.00031));
        assert!(decoded.finished);
    }

    #[test]
    fn streaming_chat_completions_extracts_delta_and_usage() {
        let stream = Cursor::new(
            br#"data: {"choices":[{"delta":{"content":"hello"},"finish_reason":null}]}

data: {"choices":[{"delta":{"content":" world"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2,"cost":0.00012}}

data: [DONE]

"#,
        );

        let decoded = decode_streaming_response(
            provider_profile("openrouter").expect("openrouter profile"),
            stream,
            4096,
            &test_tokenizer(),
        )
        .expect("decode stream");

        assert_eq!(decoded.emitted_text, "hello world");
        assert_eq!(decoded.usage.input_tokens, Some(5));
        assert_eq!(decoded.usage.output_tokens, Some(2));
        assert_eq!(decoded.usage.estimated_cost_usd, Some(0.00012));
        assert!(decoded.finished);
    }

    #[test]
    fn extract_chat_completions_text_supports_message_and_delta_shapes() {
        let message_payload = serde_json::json!({
            "choices": [{"message": {"content": "hello world"}}]
        });
        let delta_payload = serde_json::json!({
            "choices": [{"delta": {"content": "hello world"}}]
        });

        assert_eq!(
            extract_chat_completions_text(&message_payload).as_deref(),
            Some("hello world")
        );
        assert_eq!(
            extract_chat_completions_text(&delta_payload).as_deref(),
            Some("hello world")
        );
    }

    #[test]
    fn telemetry_classifies_rate_limit_and_auth_errors() {
        let _guard = telemetry_test_guard();
        reset_telemetry(None);

        record_http_error("openai-responses", 429, "gpt-4.1-mini", "rate limited");
        record_http_error("openai-responses", 401, "gpt-4.1-mini", "unauthorized");

        let telemetry = telemetry_snapshot("openai-responses").expect("openai telemetry");
        assert_eq!(telemetry.rate_limit_errors, 1);
        assert_eq!(telemetry.auth_errors, 1);
        assert_eq!(telemetry.transport_errors, 0);
        assert_eq!(telemetry.last_model.as_deref(), Some("gpt-4.1-mini"));
        assert_eq!(telemetry.last_error.as_deref(), Some("unauthorized"));
    }

    #[test]
    fn telemetry_tracks_transport_errors_per_backend() {
        let _guard = telemetry_test_guard();
        reset_telemetry(None);

        record_transport_error("openrouter", "openai/gpt-4.1-mini", "connection reset");

        let telemetry = telemetry_snapshot("openrouter").expect("openrouter telemetry");
        assert_eq!(telemetry.rate_limit_errors, 0);
        assert_eq!(telemetry.auth_errors, 0);
        assert_eq!(telemetry.transport_errors, 1);
        assert_eq!(telemetry.last_model.as_deref(), Some("openai/gpt-4.1-mini"));
        assert_eq!(telemetry.last_error.as_deref(), Some("connection reset"));
    }
}
