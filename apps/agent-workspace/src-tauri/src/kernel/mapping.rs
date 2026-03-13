use crate::models::kernel::AuditEvent;

pub fn make_audit_event(category: &str, title: &str, detail: String) -> AuditEvent {
    AuditEvent {
        category: category.to_string(),
        kind: category.to_string(),
        title: title.to_string(),
        detail,
        recorded_at_ms: 0,
        session_id: None,
        pid: None,
        runtime_id: None,
    }
}

pub fn humanize_kernel_event(raw: &str) -> String {
    raw.replace("context_strategy", "strategy")
        .replace("context_tokens_used", "tokens_used")
        .replace("context_window_size", "window")
        .replace("last_summary_ts", "last summary")
}
