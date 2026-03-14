use super::{StorageService, StoredAccountingEvent};
    use crate::accounting::AccountingEventStatus;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn accounting_summary_survives_reopen() {
        let dir = make_temp_dir("agenticos-accounting-storage");
        let db_path = dir.join("agenticos.db");

        {
            let mut storage = StorageService::open(&db_path).expect("open storage");
            storage
                .insert_session(
                    "sess-1",
                    "accounting session",
                    "idle",
                    None,
                    None,
                    1_000,
                    1_000,
                )
                .expect("insert session");
            storage
                .record_accounting_event(&StoredAccountingEvent {
                    session_id: Some("sess-1".to_string()),
                    pid: Some(11),
                    runtime_id: None,
                    backend_id: "openai-responses".to_string(),
                    backend_class: "remote_stateless".to_string(),
                    provider_id: Some("openai-responses".to_string()),
                    model_id: Some("gpt-4.1-mini".to_string()),
                    request_kind: "inference_step".to_string(),
                    status: AccountingEventStatus::Success,
                    request_count: 1,
                    stream: true,
                    input_tokens: 21,
                    output_tokens: 8,
                    estimated_cost_usd: 0.00042,
                    error_code: None,
                    error_message: None,
                })
                .expect("insert success event");
            storage
                .record_accounting_event(&StoredAccountingEvent {
                    session_id: Some("sess-1".to_string()),
                    pid: Some(11),
                    runtime_id: None,
                    backend_id: "openai-responses".to_string(),
                    backend_class: "remote_stateless".to_string(),
                    provider_id: Some("openai-responses".to_string()),
                    model_id: Some("gpt-4.1-mini".to_string()),
                    request_kind: "inference_step".to_string(),
                    status: AccountingEventStatus::RateLimitError,
                    request_count: 1,
                    stream: true,
                    input_tokens: 13,
                    output_tokens: 0,
                    estimated_cost_usd: 0.0,
                    error_code: Some("429".to_string()),
                    error_message: Some("rate limited".to_string()),
                })
                .expect("insert failure event");
            assert_eq!(storage.accounting_event_count().expect("event count"), 2);
        }

        let reopened = StorageService::open(&db_path).expect("reopen storage");
        let global = reopened
            .global_accounting_summary()
            .expect("global summary")
            .expect("global data");
        let session = reopened
            .accounting_summary_for_session("sess-1")
            .expect("session summary")
            .expect("session data");
        let backend = reopened
            .accounting_summary_for_backend("openai-responses")
            .expect("backend summary")
            .expect("backend data");

        assert_eq!(global.requests_total, 2);
        assert_eq!(global.stream_requests_total, 2);
        assert_eq!(global.input_tokens_total, 34);
        assert_eq!(global.output_tokens_total, 8);
        assert!((global.estimated_cost_usd - 0.00042).abs() < 1e-12);
        assert_eq!(global.rate_limit_errors, 1);
        assert_eq!(global.auth_errors, 0);
        assert_eq!(global.transport_errors, 0);
        assert_eq!(global.last_model.as_deref(), Some("gpt-4.1-mini"));
        assert_eq!(global.last_error.as_deref(), Some("rate limited"));
        assert_eq!(session, backend);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn accounting_summary_aggregates_multiple_providers_without_cross_contamination() {
        let dir = make_temp_dir("agenticos-accounting-multi-provider");
        let db_path = dir.join("agenticos.db");

        let mut storage = StorageService::open(&db_path).expect("open storage");
        storage
            .insert_session("sess-openai", "OpenAI session", "idle", None, None, 1, 1)
            .expect("insert openai session");
        storage
            .insert_session("sess-groq", "Groq session", "idle", None, None, 2, 2)
            .expect("insert groq session");

        storage
            .record_accounting_event(&StoredAccountingEvent {
                session_id: Some("sess-openai".to_string()),
                pid: Some(21),
                runtime_id: None,
                backend_id: "openai-responses".to_string(),
                backend_class: "remote_stateless".to_string(),
                provider_id: Some("openai-responses".to_string()),
                model_id: Some("gpt-4.1-mini".to_string()),
                request_kind: "inference_step".to_string(),
                status: AccountingEventStatus::Success,
                request_count: 1,
                stream: true,
                input_tokens: 120,
                output_tokens: 45,
                estimated_cost_usd: 0.0021,
                error_code: None,
                error_message: None,
            })
            .expect("insert openai event");
        storage
            .record_accounting_event(&StoredAccountingEvent {
                session_id: Some("sess-groq".to_string()),
                pid: Some(22),
                runtime_id: None,
                backend_id: "groq-responses".to_string(),
                backend_class: "remote_stateless".to_string(),
                provider_id: Some("groq-responses".to_string()),
                model_id: Some("llama-3.3-70b-versatile".to_string()),
                request_kind: "inference_step".to_string(),
                status: AccountingEventStatus::AuthError,
                request_count: 1,
                stream: false,
                input_tokens: 33,
                output_tokens: 0,
                estimated_cost_usd: 0.0,
                error_code: Some("401".to_string()),
                error_message: Some("invalid key".to_string()),
            })
            .expect("insert groq auth event");
        storage
            .record_accounting_event(&StoredAccountingEvent {
                session_id: Some("sess-groq".to_string()),
                pid: Some(22),
                runtime_id: None,
                backend_id: "openrouter".to_string(),
                backend_class: "remote_stateless".to_string(),
                provider_id: Some("openrouter".to_string()),
                model_id: Some("qwen/qwen3-4b:free".to_string()),
                request_kind: "inference_step".to_string(),
                status: AccountingEventStatus::TransportError,
                request_count: 1,
                stream: true,
                input_tokens: 12,
                output_tokens: 0,
                estimated_cost_usd: 0.0,
                error_code: Some("transport".to_string()),
                error_message: Some("upstream timeout".to_string()),
            })
            .expect("insert openrouter transport event");

        let global = storage
            .global_accounting_summary()
            .expect("global summary")
            .expect("global data");
        let openai = storage
            .accounting_summary_for_backend("openai-responses")
            .expect("openai summary")
            .expect("openai data");
        let groq = storage
            .accounting_summary_for_backend("groq-responses")
            .expect("groq summary")
            .expect("groq data");
        let openrouter = storage
            .accounting_summary_for_backend("openrouter")
            .expect("openrouter summary")
            .expect("openrouter data");
        let session_groq = storage
            .accounting_summary_for_session("sess-groq")
            .expect("groq session summary")
            .expect("groq session data");

        assert_eq!(global.requests_total, 3);
        assert_eq!(global.stream_requests_total, 2);
        assert_eq!(global.input_tokens_total, 165);
        assert_eq!(global.output_tokens_total, 45);
        assert!((global.estimated_cost_usd - 0.0021).abs() < 1e-12);
        assert_eq!(global.auth_errors, 1);
        assert_eq!(global.transport_errors, 1);
        assert_eq!(global.rate_limit_errors, 0);

        assert_eq!(openai.requests_total, 1);
        assert_eq!(openai.input_tokens_total, 120);
        assert_eq!(openai.output_tokens_total, 45);
        assert!((openai.estimated_cost_usd - 0.0021).abs() < 1e-12);
        assert_eq!(openai.auth_errors, 0);
        assert_eq!(openai.transport_errors, 0);

        assert_eq!(groq.requests_total, 1);
        assert_eq!(groq.stream_requests_total, 0);
        assert_eq!(groq.input_tokens_total, 33);
        assert_eq!(groq.output_tokens_total, 0);
        assert_eq!(groq.auth_errors, 1);
        assert_eq!(groq.transport_errors, 0);
        assert_eq!(groq.last_error.as_deref(), Some("invalid key"));

        assert_eq!(openrouter.requests_total, 1);
        assert_eq!(openrouter.stream_requests_total, 1);
        assert_eq!(openrouter.input_tokens_total, 12);
        assert_eq!(openrouter.output_tokens_total, 0);
        assert_eq!(openrouter.auth_errors, 0);
        assert_eq!(openrouter.transport_errors, 1);
        assert_eq!(openrouter.last_error.as_deref(), Some("upstream timeout"));

        assert_eq!(session_groq.requests_total, 2);
        assert_eq!(session_groq.stream_requests_total, 1);
        assert_eq!(session_groq.input_tokens_total, 45);
        assert_eq!(session_groq.output_tokens_total, 0);
        assert_eq!(session_groq.auth_errors, 1);
        assert_eq!(session_groq.transport_errors, 1);

        let _ = fs::remove_dir_all(dir);
    }

    fn make_temp_dir(prefix: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), timestamp));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
