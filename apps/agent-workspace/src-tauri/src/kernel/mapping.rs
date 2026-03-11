use crate::models::kernel::AuditEvent;

pub fn make_audit_event(category: &str, title: &str, detail: String) -> AuditEvent {
    AuditEvent {
        category: category.to_string(),
        title: title.to_string(),
        detail,
    }
}

pub fn humanize_kernel_event(raw: &str) -> String {
    raw.replace("context_strategy", "strategy")
        .replace("context_tokens_used", "tokens_used")
        .replace("context_window_size", "window")
        .replace("last_summary_ts", "last summary")
}
