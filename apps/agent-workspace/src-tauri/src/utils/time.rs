use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn duration_label(elapsed_secs: f64) -> String {
    let seconds = elapsed_secs.max(0.0).round() as u64;
    if seconds >= 3600 {
        format!("{}h", seconds / 3600)
    } else if seconds >= 60 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}s", seconds)
    }
}

pub(crate) fn relative_age_label(updated_at_ms: i64) -> String {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(updated_at_ms);
    let delta_secs = ((now_ms - updated_at_ms).max(0) / 1_000) as u64;
    if delta_secs >= 86_400 {
        format!("{}d", delta_secs / 86_400)
    } else if delta_secs >= 3_600 {
        format!("{}h", delta_secs / 3_600)
    } else if delta_secs >= 60 {
        format!("{}m", delta_secs / 60)
    } else {
        format!("{}s", delta_secs)
    }
}
