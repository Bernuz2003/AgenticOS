use agentic_control_models::BackendTelemetryView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccountingEventStatus {
    Success,
    RateLimitError,
    AuthError,
    TransportError,
    HttpError,
}

impl AccountingEventStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::RateLimitError => "rate_limit_error",
            Self::AuthError => "auth_error",
            Self::TransportError => "transport_error",
            Self::HttpError => "http_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BackendAccountingEvent {
    pub(crate) backend_id: String,
    pub(crate) model_id: Option<String>,
    pub(crate) request_count: u64,
    pub(crate) stream: bool,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) estimated_cost_usd: f64,
    pub(crate) duration_ms: u128,
    pub(crate) status: AccountingEventStatus,
    pub(crate) error_code: Option<String>,
    pub(crate) error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct AccountingSummary {
    pub(crate) requests_total: u64,
    pub(crate) stream_requests_total: u64,
    pub(crate) input_tokens_total: u64,
    pub(crate) output_tokens_total: u64,
    pub(crate) estimated_cost_usd: f64,
    pub(crate) rate_limit_errors: u64,
    pub(crate) auth_errors: u64,
    pub(crate) transport_errors: u64,
    pub(crate) last_model: Option<String>,
    pub(crate) last_error: Option<String>,
}

impl AccountingSummary {
    pub(crate) fn into_view(self) -> BackendTelemetryView {
        BackendTelemetryView {
            requests_total: self.requests_total,
            stream_requests_total: self.stream_requests_total,
            input_tokens_total: self.input_tokens_total,
            output_tokens_total: self.output_tokens_total,
            estimated_cost_usd: self.estimated_cost_usd,
            rate_limit_errors: self.rate_limit_errors,
            auth_errors: self.auth_errors,
            transport_errors: self.transport_errors,
            last_model: self.last_model,
            last_error: self.last_error,
        }
    }
}
