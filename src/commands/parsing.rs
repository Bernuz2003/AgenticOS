use crate::prompting::GenerationConfig;

pub(crate) fn parse_memw_payload(payload: &[u8]) -> Result<(u64, Vec<u8>), String> {
    if payload.is_empty() {
        return Err("MEMW payload empty. Use canonical format '<pid>\\n<raw-bytes>'".to_string());
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

    Err(
        "MEMW requires canonical format '<pid>\\n<raw-bytes>'; pipe syntax is no longer accepted"
            .to_string(),
    )
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

#[cfg(test)]
mod tests {
    use super::{parse_generation_payload, parse_memw_payload};
    use crate::prompting::GenerationConfig;

    fn base_gen() -> GenerationConfig {
        GenerationConfig {
            temperature: 0.7,
            top_p: 0.9,
            seed: 42,
            max_tokens: 500,
        }
    }

    #[test]
    fn parse_gen_basic_comma_separated() {
        let cfg = parse_generation_payload("temperature=0.5, top_p=0.8", base_gen()).unwrap();
        assert!((cfg.temperature - 0.5).abs() < 1e-6);
        assert!((cfg.top_p - 0.8).abs() < 1e-6);
        assert_eq!(cfg.seed, 42);
        assert_eq!(cfg.max_tokens, 500);
    }

    #[test]
    fn parse_gen_semicolon_separated() {
        let cfg = parse_generation_payload("seed=123; max_tokens=256", base_gen()).unwrap();
        assert_eq!(cfg.seed, 123);
        assert_eq!(cfg.max_tokens, 256);
    }

    #[test]
    fn parse_gen_empty_payload_errors() {
        assert!(parse_generation_payload("", base_gen()).is_err());
    }

    #[test]
    fn parse_gen_unknown_key_errors() {
        assert!(parse_generation_payload("badkey=1", base_gen()).is_err());
    }

    #[test]
    fn parse_gen_temp_out_of_range_errors() {
        assert!(parse_generation_payload("temperature=5.0", base_gen()).is_err());
    }

    #[test]
    fn parse_gen_top_p_out_of_range_errors() {
        assert!(parse_generation_payload("top_p=1.5", base_gen()).is_err());
    }

    #[test]
    fn parse_gen_max_tokens_zero_errors() {
        assert!(parse_generation_payload("max_tokens=0", base_gen()).is_err());
    }

    #[test]
    fn parse_memw_newline_format() {
        let payload = b"42\nraw data here";
        let (pid, data) = parse_memw_payload(payload).unwrap();
        assert_eq!(pid, 42);
        assert_eq!(data, b"raw data here");
    }

    #[test]
    fn parse_memw_pipe_format_rejected() {
        let payload = b"7|some text";
        assert!(parse_memw_payload(payload).is_err());
    }

    #[test]
    fn parse_memw_empty_errors() {
        assert!(parse_memw_payload(b"").is_err());
    }

    #[test]
    fn parse_memw_invalid_pid_errors() {
        assert!(parse_memw_payload(b"notanumber\ndata").is_err());
    }

    #[test]
    fn parse_memw_empty_body_after_pid_errors() {
        assert!(parse_memw_payload(b"42\n").is_err());
    }
}
