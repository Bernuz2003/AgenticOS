use super::{
    build_completion_request, decode_completion_response, select_completion_prompt_transport,
    PromptTransportStrategy,
};
use crate::backend::remote::streaming::{agent_invocation_end, drain_json_objects};
use crate::prompting::{GenerationConfig, PromptFamily};
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

#[test]
fn completion_request_contains_expected_llamacpp_fields() {
    let request = build_completion_request(
        "hello",
        32,
        Some(7),
        GenerationConfig::defaults_for(PromptFamily::Unknown),
        false,
    );

    assert_eq!(request["prompt"].as_str(), Some("hello"));
    assert_eq!(request["n_predict"].as_u64(), Some(32));
    assert_eq!(request["id_slot"].as_i64(), Some(7));
    assert_eq!(request["cache_prompt"].as_bool(), Some(true));
    assert_eq!(request["return_tokens"].as_bool(), Some(true));
    assert_eq!(request["stream"].as_bool(), Some(false));
}

#[test]
fn build_completion_request_can_enable_streaming() {
    let request = build_completion_request(
        "hello",
        8,
        Some(3),
        GenerationConfig::defaults_for(PromptFamily::Unknown),
        true,
    );

    assert_eq!(request["stream"].as_bool(), Some(true));
}

#[test]
fn transport_strategy_uses_suffix_when_backend_supports_append_only() {
    let transport = select_completion_prompt_transport("hello\nOutput:\n2\n", "Output:\n2\n", true);

    assert_eq!(
        transport.strategy,
        PromptTransportStrategy::AppendOnlySuffix
    );
    assert_eq!(transport.prompt, "Output:\n2\n");
}

#[test]
fn transport_strategy_falls_back_to_full_prompt_for_llamacpp() {
    let transport =
        select_completion_prompt_transport("hello\nOutput:\n2\n", "Output:\n2\n", false);

    assert_eq!(transport.strategy, PromptTransportStrategy::FullPrompt);
    assert_eq!(transport.prompt, "hello\nOutput:\n2\n");
}

#[test]
fn decode_captured_reasoning_payload_keeps_reasoning_separate_from_visible_text() {
    let tokenizer = test_tokenizer();
    let decoded = decode_completion_response(
        serde_json::json!({
            "content": "hello",
            "reasoning_content": "step by step",
            "tokens": [1],
            "stop": true,
            "stop_type": "eos"
        }),
        &tokenizer,
    )
    .expect("decode completion");

    assert_eq!(decoded.emitted_text, "hello");
    assert_eq!(decoded.emitted_reasoning_text, "step by step");
    assert_eq!(decoded.appended_tokens, vec![1]);
    assert!(decoded.finished);
}

#[test]
fn decode_captured_choice_payload_tokenizes_when_backend_omits_tokens() {
    let tokenizer = test_tokenizer();
    let decoded = decode_completion_response(
        serde_json::json!({
            "choices": [
                {
                    "text": "hello world",
                    "finish_reason": "stop"
                }
            ]
        }),
        &tokenizer,
    )
    .expect("decode completion");

    assert_eq!(decoded.emitted_text, "hello world");
    assert_eq!(decoded.appended_tokens, vec![1, 2]);
    assert!(decoded.finished);
}

#[test]
fn decode_limit_response_does_not_report_finished() {
    let tokenizer = test_tokenizer();
    let decoded = decode_completion_response(
        serde_json::json!({
            "content": "hello",
            "tokens": [1],
            "stop": true,
            "stop_type": "limit",
            "truncated": true,
            "choices": [
                {
                    "finish_reason": "length"
                }
            ]
        }),
        &tokenizer,
    )
    .expect("decode completion");

    assert!(!decoded.finished);
}

#[test]
fn drain_json_objects_extracts_sse_prefixed_payloads() {
    let mut buffer =
            b"data: {\"content\":\"hello\",\"stop\":false}\n\ndata: {\"content\":\"world\",\"stop\":true}\n\n"
                .to_vec();
    let values = drain_json_objects(&mut buffer).expect("drain stream objects");

    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["content"].as_str(), Some("hello"));
    assert_eq!(values[1]["content"].as_str(), Some("world"));
    assert!(buffer.is_empty());
}

#[test]
fn agent_invocation_end_detects_complete_canonical_line() {
    let stream = "Prelude\nTOOL:calc {\"expression\":\"1+1\"}";
    let end = agent_invocation_end(stream).expect("agent invocation marker");

    assert_eq!(&stream[..end], stream);
}

#[test]
fn agent_invocation_end_detects_action_line() {
    let stream = "Prelude\nACTION:send {\"pid\":42,\"message\":\"hello\"}";
    let end = agent_invocation_end(stream).expect("action marker");

    assert_eq!(&stream[..end], stream);
}

#[test]
fn agent_invocation_end_truncates_tool_suffix_after_json_boundary() {
    let stream = "Prelude\nTOOL:calc {\"expression\":\"1+1\"} trailing text";
    let end = agent_invocation_end(stream).expect("tool marker");

    assert_eq!(
        &stream[..end],
        "Prelude\nTOOL:calc {\"expression\":\"1+1\"}"
    );
}

#[test]
fn agent_invocation_end_truncates_action_suffix_after_json_boundary() {
    let stream = "Prelude\nACTION:send {\"pid\":42,\"message\":\"hello\"} trailing text";
    let end = agent_invocation_end(stream).expect("action marker");

    assert_eq!(
        &stream[..end],
        "Prelude\nACTION:send {\"pid\":42,\"message\":\"hello\"}"
    );
}

#[test]
fn agent_invocation_end_detects_inline_tool_invocation() {
    let stream = "Creo la cartella richiesta: TOOL:mkdir {\"path\":\"prova\"}";
    let end = agent_invocation_end(stream).expect("tool marker");

    assert_eq!(&stream[..end], stream);
}

#[test]
fn agent_invocation_end_skips_invalid_inline_mention_before_real_invocation() {
    let stream = "Uso la funzione TOOL:mkdir. Ecco la richiesta:\nTOOL:mkdir {\"path\":\"prova\"}";
    let end = agent_invocation_end(stream).expect("tool marker");

    assert_eq!(&stream[..end], stream);
}
