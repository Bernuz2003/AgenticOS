use crate::prompting::GenerationConfig;

pub(crate) fn parse_memw_payload(payload: &[u8]) -> Result<(u64, Vec<u8>), String> {
    if payload.is_empty() {
        return Err("MEMW payload empty. Use '<pid>\\n<raw-bytes>' or '<pid>|<text>'".to_string());
    }

    if let Some(pos) = payload.iter().position(|b| *b == b'\n') {
        let pid_str = String::from_utf8_lossy(&payload[..pos]).trim().to_string();
        let pid: u64 = pid_str
            .parse()
            .map_err(|_| format!("Invalid PID '{}'.", pid_str))?;
        let raw = payload[pos + 1..].to_vec();
        if raw.is_empty() {
            return Err("MEMW raw bytes are empty after PID header".to_string());
        }
        return Ok((pid, raw));
    }

    let text = String::from_utf8(payload.to_vec())
        .map_err(|_| "MEMW payload must be valid UTF-8 when using pipe format".to_string())?;
    let mut parts = text.splitn(2, '|');
    let pid_str = parts.next().unwrap_or("").trim();
    let body = parts
        .next()
        .ok_or_else(|| "MEMW pipe format requires '<pid>|<text>'".to_string())?;

    let pid: u64 = pid_str
        .parse()
        .map_err(|_| format!("Invalid PID '{}'.", pid_str))?;

    Ok((pid, body.as_bytes().to_vec()))
}

pub(crate) fn parse_generation_payload(
    payload: &str,
    base: GenerationConfig,
) -> Result<GenerationConfig, String> {
    if payload.is_empty() {
        return Err("SET_GEN payload is empty. Use key=value pairs.".to_string());
    }

    let mut cfg = base;

    for pair in payload.split([',', ';']) {
        let item = pair.trim();
        if item.is_empty() {
            continue;
        }

        let mut it = item.splitn(2, '=');
        let key = it.next().unwrap_or("").trim().to_lowercase();
        let value = it
            .next()
            .ok_or_else(|| format!("Invalid item '{}'. Expected key=value", item))?
            .trim();

        match key.as_str() {
            "temperature" | "temp" => {
                let parsed: f64 = value
                    .parse()
                    .map_err(|_| format!("Invalid temperature '{}'.", value))?;
                if !(0.0..=2.0).contains(&parsed) {
                    return Err("temperature must be in [0.0, 2.0]".to_string());
                }
                cfg.temperature = parsed;
            }
            "top_p" | "topp" => {
                let parsed: f64 = value
                    .parse()
                    .map_err(|_| format!("Invalid top_p '{}'.", value))?;
                if !(0.0..=1.0).contains(&parsed) {
                    return Err("top_p must be in [0.0, 1.0]".to_string());
                }
                cfg.top_p = parsed;
            }
            "seed" => {
                cfg.seed = value
                    .parse()
                    .map_err(|_| format!("Invalid seed '{}'.", value))?;
            }
            "max_tokens" | "max_new_tokens" => {
                let parsed: usize = value
                    .parse()
                    .map_err(|_| format!("Invalid max_tokens '{}'.", value))?;
                if parsed == 0 {
                    return Err("max_tokens must be > 0".to_string());
                }
                cfg.max_tokens = parsed;
            }
            _ => return Err(format!("Unknown SET_GEN key '{}'.", key)),
        }
    }

    Ok(cfg)
}
