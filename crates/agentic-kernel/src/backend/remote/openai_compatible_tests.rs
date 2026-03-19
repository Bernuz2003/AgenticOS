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
        None,
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
        None,
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
