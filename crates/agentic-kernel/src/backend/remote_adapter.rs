use anyhow::{Error as E, Result};
use serde::Deserialize;
use serde_json::json;
use tokenizers::Tokenizer;

use crate::memory::ContextSlotId;
use crate::prompting::GenerationConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptTransportStrategy {
    FullPrompt,
    AppendOnlySuffix,
}

pub(crate) struct CompletionPromptTransport<'a> {
    pub(crate) strategy: PromptTransportStrategy,
    pub(crate) prompt: &'a str,
}

pub(crate) fn select_completion_prompt_transport<'a>(
    rendered_prompt: &'a str,
    resident_prompt_suffix: &'a str,
    append_only_supported: bool,
) -> CompletionPromptTransport<'a> {
    if append_only_supported && !resident_prompt_suffix.is_empty() {
        CompletionPromptTransport {
            strategy: PromptTransportStrategy::AppendOnlySuffix,
            prompt: resident_prompt_suffix,
        }
    } else {
        CompletionPromptTransport {
            strategy: PromptTransportStrategy::FullPrompt,
            prompt: rendered_prompt,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct CompletionChoice {
    pub(crate) text: Option<String>,
    pub(crate) finish_reason: Option<String>,
    pub(crate) reasoning_content: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CompletionResponse {
    pub(crate) content: Option<String>,
    pub(crate) reasoning_content: Option<String>,
    pub(crate) tokens: Option<Vec<u32>>,
    pub(crate) stop: Option<bool>,
    pub(crate) stop_type: Option<String>,
    pub(crate) stopping_word: Option<String>,
    pub(crate) stopped_eos: Option<bool>,
    pub(crate) stopped_word: Option<bool>,
    pub(crate) truncated: Option<bool>,
    pub(crate) choices: Option<Vec<CompletionChoice>>,
}

pub(crate) struct DecodedCompletion {
    pub(crate) emitted_text: String,
    #[allow(dead_code)]
    pub(crate) appended_tokens: Vec<u32>,
    pub(crate) finished: bool,
}

pub(crate) fn build_completion_request(
    prompt: &str,
    chunk_tokens: usize,
    context_slot_id: Option<ContextSlotId>,
    generation: GenerationConfig,
    stream: bool,
) -> serde_json::Value {
    json!({
        "prompt": prompt,
        "n_predict": chunk_tokens,
        "id_slot": context_slot_id.map(slot_id_to_i32).unwrap_or(-1),
        "temperature": generation.temperature,
        "top_p": generation.top_p,
        "seed": generation.seed,
        "cache_prompt": true,
        "return_tokens": true,
        "stream": stream,
    })
}

pub(crate) fn decode_completion_response(
    raw: serde_json::Value,
    tokenizer: &Tokenizer,
) -> Result<DecodedCompletion> {
    let response: CompletionResponse = serde_json::from_value(raw).map_err(|e| {
        E::msg(format!(
            "Malformed completion payload from external RPC backend: {}",
            e
        ))
    })?;

    let choice = response
        .choices
        .as_ref()
        .and_then(|choices| choices.first());
    let emitted_text = combine_completion_text(
        response.reasoning_content.as_deref(),
        response.content.as_deref(),
        choice.and_then(|item| item.reasoning_content.as_deref()),
        choice.and_then(|item| item.text.as_deref()),
    );
    let finish_reason = choice.and_then(|item| item.finish_reason.as_deref());
    let finished = completion_is_finished(&response, finish_reason);
    let appended_tokens = if let Some(tokens) = response.tokens {
        tokens
    } else if emitted_text.is_empty() {
        Vec::new()
    } else {
        tokenizer
            .encode(emitted_text.as_str(), false)
            .map_err(|e| E::msg(format!("Failed to tokenize RPC completion chunk: {}", e)))?
            .get_ids()
            .to_vec()
    };

    Ok(DecodedCompletion {
        emitted_text,
        appended_tokens,
        finished,
    })
}

fn wrap_reasoning_content(reasoning: &str) -> String {
    format!("<think>\n{}\n</think>", reasoning)
}

pub(crate) fn combine_completion_text(
    reasoning_content: Option<&str>,
    content: Option<&str>,
    choice_reasoning_content: Option<&str>,
    choice_text: Option<&str>,
) -> String {
    let content = content.or(choice_text).unwrap_or_default();
    let reasoning = reasoning_content
        .or(choice_reasoning_content)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match reasoning {
        Some(reasoning) if !content.contains("<think>") => {
            if content.is_empty() {
                wrap_reasoning_content(reasoning)
            } else {
                format!("{}\n{}", wrap_reasoning_content(reasoning), content)
            }
        }
        _ => content.to_string(),
    }
}

pub(crate) fn completion_is_finished(
    response: &CompletionResponse,
    finish_reason: Option<&str>,
) -> bool {
    let stop_type = response.stop_type.as_deref();
    let hit_length_limit = matches!(stop_type, Some("limit"))
        || matches!(finish_reason, Some("length"))
        || response.truncated.unwrap_or(false);

    matches!(stop_type, Some("eos") | Some("word"))
        || response.stopped_eos.unwrap_or(false)
        || response.stopped_word.unwrap_or(false)
        || matches!(finish_reason, Some("stop"))
        || response
            .stopping_word
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        || (response.stop.unwrap_or(false) && response.stop_type.is_none() && !hit_length_limit)
}

fn slot_id_to_i32(slot_id: ContextSlotId) -> i32 {
    i32::try_from(slot_id).unwrap_or(i32::MAX)
}

pub(crate) fn drain_json_objects(buffer: &mut Vec<u8>) -> Result<Vec<serde_json::Value>> {
    let mut objects = Vec::new();
    let mut cursor = 0usize;

    loop {
        let Some(start_rel) = buffer[cursor..].iter().position(|byte| *byte == b'{') else {
            if cursor > 0 {
                if buffer[cursor..]
                    .iter()
                    .all(|byte| byte.is_ascii_whitespace())
                {
                    buffer.clear();
                } else {
                    buffer.drain(..cursor);
                }
            } else if !buffer.is_empty() && buffer.iter().all(|byte| byte.is_ascii_whitespace()) {
                buffer.clear();
            }
            return Ok(objects);
        };
        let start = cursor + start_rel;
        let Some(end_rel) = find_complete_json_object_end(&buffer[start..]) else {
            if start > 0 {
                buffer.drain(..start);
            }
            return Ok(objects);
        };
        let end = start + end_rel + 1;
        objects.push(serde_json::from_slice(&buffer[start..end]).map_err(|err| {
            E::msg(format!(
                "Malformed streaming completion payload from external RPC backend: {}",
                err
            ))
        })?);
        cursor = end;
    }
}

pub(crate) fn tool_invocation_end(stream: &str) -> Option<usize> {
    if let Some(start) = stream.find("[[") {
        let rest = &stream[start + 2..];
        if let Some(end_offset) = rest.find("]]") {
            let candidate = rest[..end_offset].trim();
            if crate::tools::validates_tool_invocation(candidate) {
                return Some(start + 2 + end_offset + 2);
            }
        }
    }

    let mut offset = 0usize;
    for line in stream.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("TOOL:") && crate::tools::validates_tool_invocation(trimmed) {
            return Some(offset + line.len());
        }
        offset += line.len();
    }

    let last_line_start = stream.rfind('\n').map(|index| index + 1).unwrap_or(0);
    let last_line = stream[last_line_start..].trim();
    if last_line.starts_with("TOOL:") && crate::tools::validates_tool_invocation(last_line) {
        return Some(stream.len());
    }

    None
}

fn find_complete_json_object_end(bytes: &[u8]) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, byte) in bytes.iter().enumerate() {
        match *byte {
            b'\\' if in_string && !escaped => escaped = true,
            b'"' if !escaped => in_string = !in_string,
            b'{' if !in_string => depth += 1,
            b'}' if !in_string => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => escaped = false,
        }

        if *byte != b'\\' {
            escaped = false;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        build_completion_request, decode_completion_response, drain_json_objects,
        select_completion_prompt_transport, tool_invocation_end, PromptTransportStrategy,
    };
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
        let transport =
            select_completion_prompt_transport("hello\nOutput:\n2\n", "Output:\n2\n", true);

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
    fn decode_captured_reasoning_payload_combines_reasoning_and_content() {
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

        assert!(decoded.emitted_text.contains("<think>"));
        assert!(decoded.emitted_text.contains("hello"));
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
    fn tool_invocation_end_detects_complete_canonical_line() {
        let stream = "Prelude\nTOOL:calc {\"expression\":\"1+1\"}";
        let end = tool_invocation_end(stream).expect("tool marker");

        assert_eq!(&stream[..end], stream);
    }
}
