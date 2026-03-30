use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExtractedPrefixedInvocation {
    pub(crate) name: String,
    pub(crate) input: Value,
    pub(crate) raw_invocation: String,
    pub(crate) consumed_bytes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PrefixedInvocationExtract {
    Parsed(ExtractedPrefixedInvocation),
    Incomplete,
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LocatedPrefixedInvocation {
    pub(crate) start_offset: usize,
    pub(crate) parsed: ExtractedPrefixedInvocation,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PrefixedInvocationSearch {
    Parsed(LocatedPrefixedInvocation),
    Incomplete { start_offset: usize },
    NotFound,
}

pub(crate) fn parse_prefixed_json_invocation(
    text: &str,
    prefix: &str,
) -> Result<(String, Value), String> {
    let clean_text = text.trim();
    if clean_text.contains('\n') || clean_text.contains('\r') {
        return Err("Invocation must fit on a single line.".to_string());
    }

    match extract_prefixed_json_invocation(clean_text, prefix) {
        PrefixedInvocationExtract::Parsed(parsed) => {
            let trailing = clean_text[parsed.consumed_bytes..].trim();
            if trailing.is_empty() {
                Ok((parsed.name, parsed.input))
            } else {
                Err("Invocation contains trailing characters after JSON payload.".to_string())
            }
        }
        PrefixedInvocationExtract::Incomplete => {
            Err("Missing JSON payload. '{}' is required even if empty.".to_string())
        }
        PrefixedInvocationExtract::Invalid(err) => Err(err),
    }
}

#[allow(dead_code)]
pub(crate) fn is_streaming_prefixed_json_invocation(text: &str, prefix: &str) -> bool {
    matches!(
        extract_prefixed_json_invocation(text.trim_start(), prefix),
        PrefixedInvocationExtract::Incomplete
    )
}

pub(crate) fn extract_prefixed_json_invocation(
    text: &str,
    prefix: &str,
) -> PrefixedInvocationExtract {
    let Some(rest_with_ws) = text.strip_prefix(prefix) else {
        return PrefixedInvocationExtract::Invalid(format!(
            "Invocation must start with '{prefix}'"
        ));
    };

    let rest = rest_with_ws.trim_start();
    let leading_ws_after_prefix = rest_with_ws.len() - rest.len();
    let Some(separator_idx) = rest.find(|c: char| c.is_whitespace() || c == '{') else {
        return PrefixedInvocationExtract::Incomplete;
    };

    let name = &rest[..separator_idx];
    if name.is_empty() {
        return PrefixedInvocationExtract::Invalid("Invocation name cannot be empty.".to_string());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-' | '.'))
    {
        return PrefixedInvocationExtract::Invalid(format!(
            "Invocation name '{name}' is not canonical. Allowed characters: a-z, 0-9, '_', '-', '.'."
        ));
    }

    let payload_with_ws = &rest[separator_idx..];
    let payload = payload_with_ws.trim_start();
    let ws_before_payload = payload_with_ws.len() - payload.len();
    if payload.is_empty() {
        return PrefixedInvocationExtract::Incomplete;
    }
    if !payload.starts_with('{') {
        return PrefixedInvocationExtract::Invalid(
            "Missing JSON payload. '{}' is required even if empty.".to_string(),
        );
    }

    let Some(json_end_rel) = first_balanced_json_object_end(payload) else {
        return PrefixedInvocationExtract::Incomplete;
    };

    let json_str = &payload[..json_end_rel];
    let input: Value = match serde_json::from_str(json_str) {
        Ok(value) => value,
        Err(err) => {
            return PrefixedInvocationExtract::Invalid(format!("Invalid JSON payload: {err}"));
        }
    };
    if !input.is_object() {
        return PrefixedInvocationExtract::Invalid(
            "Invocation payload must be a JSON object.".to_string(),
        );
    }

    let consumed_bytes =
        prefix.len() + leading_ws_after_prefix + separator_idx + ws_before_payload + json_end_rel;

    PrefixedInvocationExtract::Parsed(ExtractedPrefixedInvocation {
        name: name.to_string(),
        input,
        raw_invocation: text[..consumed_bytes].to_string(),
        consumed_bytes,
    })
}

pub(crate) fn find_first_prefixed_json_invocation(
    text: &str,
    prefixes: &[&str],
) -> PrefixedInvocationSearch {
    let mut search_offset = 0usize;
    let mut earliest_incomplete = None;

    while search_offset < text.len() {
        let Some((marker_offset, prefix)) = next_prefix_marker(text, search_offset, prefixes)
        else {
            break;
        };

        match extract_prefixed_json_invocation(&text[marker_offset..], prefix) {
            PrefixedInvocationExtract::Parsed(parsed) => {
                if let Some(start_offset) = earliest_incomplete {
                    return PrefixedInvocationSearch::Incomplete { start_offset };
                }
                return PrefixedInvocationSearch::Parsed(LocatedPrefixedInvocation {
                    start_offset: marker_offset,
                    parsed,
                });
            }
            PrefixedInvocationExtract::Incomplete => {
                earliest_incomplete.get_or_insert(marker_offset);
            }
            PrefixedInvocationExtract::Invalid(_) => {}
        }

        search_offset = marker_offset + prefix.len();
    }

    if let Some(start_offset) = earliest_incomplete {
        PrefixedInvocationSearch::Incomplete { start_offset }
    } else {
        PrefixedInvocationSearch::NotFound
    }
}

fn next_prefix_marker<'a>(
    text: &str,
    search_offset: usize,
    prefixes: &'a [&str],
) -> Option<(usize, &'a str)> {
    prefixes
        .iter()
        .filter_map(|prefix| {
            text[search_offset..]
                .find(prefix)
                .map(|relative| (search_offset + relative, *prefix))
        })
        .min_by_key(|(offset, _)| *offset)
}

fn first_balanced_json_object_end(payload: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in payload.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(idx + ch.len_utf8());
                }
            }
            _ => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        extract_prefixed_json_invocation, find_first_prefixed_json_invocation,
        is_streaming_prefixed_json_invocation, parse_prefixed_json_invocation,
        PrefixedInvocationExtract, PrefixedInvocationSearch,
    };

    #[test]
    fn extract_stops_at_first_balanced_object() {
        let text = r#"TOOL:python {"code":"print(1)"}La sequenza"#;
        let parsed = extract_prefixed_json_invocation(text, "TOOL:");
        let PrefixedInvocationExtract::Parsed(parsed) = parsed else {
            panic!("expected parsed invocation");
        };
        assert_eq!(parsed.name, "python");
        assert_eq!(parsed.input, json!({"code": "print(1)"}));
        assert_eq!(parsed.raw_invocation, r#"TOOL:python {"code":"print(1)"}"#);
        assert_eq!(&text[parsed.consumed_bytes..], "La sequenza");
    }

    #[test]
    fn extract_handles_nested_braces_and_escapes() {
        let text = r#"ACTION:spawn {"prompt":"say \"hi\"","meta":{"a":1}}ACTION:send {"pid":1,"message":"ok"}"#;
        let parsed = extract_prefixed_json_invocation(text, "ACTION:");
        let PrefixedInvocationExtract::Parsed(parsed) = parsed else {
            panic!("expected parsed invocation");
        };
        assert_eq!(parsed.name, "spawn");
        assert_eq!(
            &text[parsed.consumed_bytes..],
            r#"ACTION:send {"pid":1,"message":"ok"}"#
        );
    }

    #[test]
    fn detects_incomplete_streaming_payload() {
        assert!(is_streaming_prefixed_json_invocation(
            r#"TOOL:python {"code":"print(1)""#,
            "TOOL:"
        ));
    }

    #[test]
    fn strict_parse_rejects_trailing_text() {
        let err =
            parse_prefixed_json_invocation(r#"TOOL:python {"code":"print(1)"}extra"#, "TOOL:")
                .expect_err("expected trailing-text rejection");
        assert!(err.contains("trailing characters"));
    }

    #[test]
    fn finds_first_inline_prefixed_invocation() {
        let text = r#"Per creare la cartella: TOOL:mkdir {"path":"prova"}"#;

        let search = find_first_prefixed_json_invocation(text, &["ACTION:", "TOOL:"]);
        let PrefixedInvocationSearch::Parsed(found) = search else {
            panic!("expected parsed invocation");
        };

        assert_eq!(found.start_offset, "Per creare la cartella: ".len());
        assert_eq!(
            found.parsed.raw_invocation,
            r#"TOOL:mkdir {"path":"prova"}"#
        );
    }

    #[test]
    fn skips_invalid_mention_before_later_valid_invocation() {
        let text = r#"Usero' la funzione TOOL:mkdir. Ecco la richiesta:

TOOL:mkdir {"path":"prova"}"#;

        let search = find_first_prefixed_json_invocation(text, &["ACTION:", "TOOL:"]);
        let PrefixedInvocationSearch::Parsed(found) = search else {
            panic!("expected parsed invocation");
        };

        assert_eq!(
            found.parsed.raw_invocation,
            r#"TOOL:mkdir {"path":"prova"}"#
        );
        assert!(text[..found.start_offset].contains("funzione TOOL:mkdir."));
    }

    #[test]
    fn returns_incomplete_for_partial_inline_invocation() {
        let text = r#"Per creare la cartella: TOOL:mkdir {"path":"prova""#;

        let search = find_first_prefixed_json_invocation(text, &["ACTION:", "TOOL:"]);
        assert_eq!(
            search,
            PrefixedInvocationSearch::Incomplete {
                start_offset: "Per creare la cartella: ".len(),
            }
        );
    }
}
