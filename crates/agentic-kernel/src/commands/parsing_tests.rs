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
