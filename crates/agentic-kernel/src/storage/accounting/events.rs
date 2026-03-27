use rusqlite::{params, params_from_iter, OptionalExtension};

use crate::services::accounting::{AccountingEventStatus, AccountingSummary};

use crate::storage::{current_timestamp_ms, StorageError, StorageService};

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
#[path = "tests/events.rs"]
mod tests;
