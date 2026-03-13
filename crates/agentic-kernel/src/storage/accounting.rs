use rusqlite::{params, params_from_iter, OptionalExtension};

use crate::accounting::{AccountingEventStatus, AccountingSummary};

use super::service::{current_timestamp_ms, StorageError, StorageService};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredAccountingEvent {
    pub(crate) session_id: Option<String>,
    pub(crate) pid: Option<u64>,
    pub(crate) runtime_id: Option<String>,
    pub(crate) backend_id: String,
    pub(crate) backend_class: String,
    pub(crate) provider_id: Option<String>,
    pub(crate) model_id: Option<String>,
    pub(crate) request_kind: String,
    pub(crate) status: AccountingEventStatus,
    pub(crate) request_count: u64,
    pub(crate) stream: bool,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) estimated_cost_usd: f64,
    pub(crate) error_code: Option<String>,
    pub(crate) error_message: Option<String>,
}

impl StorageService {
    pub(crate) fn record_accounting_event(
        &mut self,
        record: &StoredAccountingEvent,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            r#"
            INSERT INTO accounting_events (
                recorded_at_ms,
                session_id,
                pid,
                runtime_id,
                backend_id,
                backend_class,
                provider_id,
                model_id,
                request_kind,
                status,
                request_count,
                stream,
                input_tokens,
                output_tokens,
                estimated_cost_usd,
                error_code,
                error_message
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
            "#,
            params![
                current_timestamp_ms(),
                record.session_id,
                record.pid,
                record.runtime_id,
                record.backend_id,
                record.backend_class,
                record.provider_id,
                record.model_id,
                record.request_kind,
                record.status.as_str(),
                record.request_count,
                if record.stream { 1 } else { 0 },
                record.input_tokens,
                record.output_tokens,
                record.estimated_cost_usd,
                record.error_code,
                record.error_message,
            ],
        )?;

        Ok(())
    }

    pub(crate) fn global_accounting_summary(
        &self,
    ) -> Result<Option<AccountingSummary>, StorageError> {
        load_summary(&self.connection, "", [], "", [])
    }

    pub(crate) fn accounting_summary_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<AccountingSummary>, StorageError> {
        load_summary(
            &self.connection,
            "WHERE session_id = ?1",
            [session_id],
            "WHERE session_id = ?1",
            [session_id],
        )
    }

    pub(crate) fn accounting_summary_for_backend(
        &self,
        backend_id: &str,
    ) -> Result<Option<AccountingSummary>, StorageError> {
        load_summary(
            &self.connection,
            "WHERE backend_id = ?1",
            [backend_id],
            "WHERE backend_id = ?1",
            [backend_id],
        )
    }

    #[cfg(test)]
    pub(crate) fn accounting_event_count(&self) -> Result<i64, StorageError> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM accounting_events", [], |row| {
                row.get(0)
            })?)
    }
}

fn load_summary<const N: usize, const M: usize>(
    connection: &rusqlite::Connection,
    summary_filter: &str,
    summary_params: [&str; N],
    latest_filter: &str,
    latest_params: [&str; M],
) -> Result<Option<AccountingSummary>, StorageError> {
    let summary_sql = format!(
        r#"
        SELECT
            COUNT(*) AS event_count,
            COALESCE(SUM(request_count), 0),
            COALESCE(SUM(CASE WHEN stream != 0 THEN request_count ELSE 0 END), 0),
            COALESCE(SUM(input_tokens), 0),
            COALESCE(SUM(output_tokens), 0),
            COALESCE(SUM(estimated_cost_usd), 0.0),
            COALESCE(SUM(CASE WHEN status = 'rate_limit_error' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN status = 'auth_error' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN status IN ('transport_error', 'http_error') THEN 1 ELSE 0 END), 0)
        FROM accounting_events
        {summary_filter}
        "#
    );
    let latest_model_sql = format!(
        r#"
        SELECT model_id
        FROM accounting_events
        {latest_filter}
        ORDER BY recorded_at_ms DESC, event_id DESC
        LIMIT 1
        "#
    );
    let latest_error_sql = format!(
        r#"
        SELECT error_message
        FROM accounting_events
        {latest_filter}
        ORDER BY recorded_at_ms DESC, event_id DESC
        LIMIT 1
        "#
    );

    let (
        event_count,
        requests_total,
        stream_requests_total,
        input_tokens_total,
        output_tokens_total,
        estimated_cost_usd,
        rate_limit_errors,
        auth_errors,
        transport_errors,
    ): (i64, i64, i64, i64, i64, f64, i64, i64, i64) = connection.query_row(
        &summary_sql,
        params_from_iter(summary_params.iter()),
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ))
        },
    )?;

    if event_count == 0 {
        return Ok(None);
    }

    let last_model = connection
        .query_row(
            &latest_model_sql,
            params_from_iter(latest_params.iter()),
            |row| row.get(0),
        )
        .optional()?;
    let latest_error_sql = if latest_filter.is_empty() {
        latest_error_sql.replacen(
            "ORDER BY",
            "WHERE error_message IS NOT NULL\n        ORDER BY",
            1,
        )
    } else {
        latest_error_sql.replacen(
            "ORDER BY",
            "AND error_message IS NOT NULL\n        ORDER BY",
            1,
        )
    };
    let last_error = connection
        .query_row(
            &latest_error_sql,
            params_from_iter(latest_params.iter()),
            |row| row.get(0),
        )
        .optional()?;

    Ok(Some(AccountingSummary {
        requests_total: requests_total.max(0) as u64,
        stream_requests_total: stream_requests_total.max(0) as u64,
        input_tokens_total: input_tokens_total.max(0) as u64,
        output_tokens_total: output_tokens_total.max(0) as u64,
        estimated_cost_usd,
        rate_limit_errors: rate_limit_errors.max(0) as u64,
        auth_errors: auth_errors.max(0) as u64,
        transport_errors: transport_errors.max(0) as u64,
        last_model,
        last_error,
    }))
}

#[cfg(test)]
mod tests {
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
}
