use super::truncate_output;
use crate::config::kernel_config;

#[test]
fn truncate_output_preserves_utf8_boundaries() {
    let limit = kernel_config().tools.output_truncate_len;
    let text = format!("{}😀", "a".repeat(limit));
    let truncated = truncate_output(&text);
    assert!(truncated.starts_with(&"a".repeat(limit)));
    assert!(truncated.ends_with("... (Output Truncated)"));
    assert!(!truncated.contains('😀'));
}
