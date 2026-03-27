pub(crate) fn compact_token_count(tokens: usize) -> String {
    if tokens >= 1000 {
        format!("{:.1}k", (tokens as f64) / 1000.0)
    } else {
        tokens.to_string()
    }
}
