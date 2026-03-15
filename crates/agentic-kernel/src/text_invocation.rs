use serde_json::Value;

pub(crate) fn parse_prefixed_json_invocation(
    text: &str,
    prefix: &str,
) -> Result<(String, Value), String> {
    let clean_text = text.trim();

    if clean_text.contains('\n') || clean_text.contains('\r') {
        return Err("Invocation must fit on a single line.".to_string());
    }

    let Some(rest) = clean_text.strip_prefix(prefix) else {
        return Err(format!("Invocation must start with '{prefix}'"));
    };

    let rest = rest.trim_start();
    let (name, json_str) =
        if let Some(separator_idx) = rest.find(|c: char| c.is_whitespace() || c == '{') {
            let (name, payload) = rest.split_at(separator_idx);
            (name, payload.trim())
        } else {
            return Err("Missing JSON payload. '{}' is required even if empty.".to_string());
        };

    if name.is_empty() {
        return Err("Invocation name cannot be empty.".to_string());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(format!(
            "Invocation name '{name}' is not canonical. Allowed characters: a-z, 0-9, '_', '-', '.'."
        ));
    }

    if json_str.is_empty() {
        return Err("Missing JSON payload. '{}' is required.".to_string());
    }

    let input: Value =
        serde_json::from_str(json_str).map_err(|err| format!("Invalid JSON payload: {err}"))?;
    if !input.is_object() {
        return Err("Invocation payload must be a JSON object.".to_string());
    }

    Ok((name.to_string(), input))
}

pub(crate) fn is_streaming_prefixed_json_invocation(text: &str, prefix: &str) -> bool {
    let clean = text.trim_start();
    if !clean.starts_with(prefix) {
        return false;
    }

    let line = clean
        .split_once('\n')
        .map(|(first_line, _)| first_line)
        .unwrap_or(clean)
        .trim_end_matches('\r');
    let rest = line[prefix.len()..].trim_start();
    let Some(separator_idx) = rest.find(|c: char| c.is_whitespace() || c == '{') else {
        return true;
    };

    let json_str = rest[separator_idx..].trim_start();
    if json_str.is_empty() {
        return true;
    }

    match serde_json::from_str::<Value>(json_str) {
        Ok(_) => false,
        Err(err) => err.is_eof(),
    }
}
