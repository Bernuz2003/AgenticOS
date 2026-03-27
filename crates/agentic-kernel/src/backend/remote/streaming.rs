use anyhow::{Error as E, Result};

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

pub(crate) fn agent_invocation_end(stream: &str) -> Option<usize> {
    let mut offset = 0usize;
    for line in stream.split_inclusive('\n') {
        let trimmed = line.trim();
        let is_action = trimmed.starts_with("ACTION:")
            && crate::runtime::actions::parse_text_invocation(trimmed).is_ok();
        let is_tool =
            trimmed.starts_with("TOOL:") && crate::tools::validates_tool_invocation(trimmed);
        if is_action || is_tool {
            return Some(offset + line.len());
        }
        offset += line.len();
    }

    let last_line_start = stream.rfind('\n').map(|index| index + 1).unwrap_or(0);
    let last_line = stream[last_line_start..].trim();
    let is_action = last_line.starts_with("ACTION:")
        && crate::runtime::actions::parse_text_invocation(last_line).is_ok();
    let is_tool =
        last_line.starts_with("TOOL:") && crate::tools::validates_tool_invocation(last_line);
    if is_action || is_tool {
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
