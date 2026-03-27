use chrono::{DateTime, Datelike, Duration as ChronoDuration, Timelike, Utc};
use serde::Deserialize;
use std::collections::BTreeSet;

use super::scheduler::{CronField, CronSchedule, ScheduledJobState, ScheduledJobTrigger, ScheduledJobTriggerInput};

impl ScheduledJobTrigger {
    pub(crate) fn from_request(
        request: ScheduledJobTriggerInput,
        now_ms: i64,
    ) -> Result<Self, String> {
        match request {
            ScheduledJobTriggerInput::At { at_ms } => {
                if at_ms <= now_ms {
                    return Err("at_ms must be in the future".to_string());
                }
                Ok(Self::At { at_ms })
            }
            ScheduledJobTriggerInput::Interval {
                every_ms,
                starts_at_ms,
            } => {
                if every_ms == 0 {
                    return Err("every_ms must be > 0".to_string());
                }
                Ok(Self::Interval {
                    every_ms,
                    anchor_ms: starts_at_ms.unwrap_or(now_ms),
                })
            }
            ScheduledJobTriggerInput::Cron { expression } => {
                let schedule = CronSchedule::parse(&expression)?;
                Ok(Self::Cron {
                    expression,
                    schedule,
                })
            }
        }
    }

    pub(crate) fn from_stored(kind: &str, payload: &str) -> Result<Self, String> {
        match kind {
            "at" => {
                #[derive(Deserialize)]
                struct AtPayload {
                    at_ms: i64,
                }
                let parsed =
                    serde_json::from_str::<AtPayload>(payload).map_err(|err| err.to_string())?;
                Ok(Self::At {
                    at_ms: parsed.at_ms,
                })
            }
            "interval" => {
                #[derive(Deserialize)]
                struct IntervalPayload {
                    every_ms: u64,
                    anchor_ms: i64,
                }
                let parsed = serde_json::from_str::<IntervalPayload>(payload)
                    .map_err(|err| err.to_string())?;
                Ok(Self::Interval {
                    every_ms: parsed.every_ms.max(1),
                    anchor_ms: parsed.anchor_ms,
                })
            }
            "cron" => {
                #[derive(Deserialize)]
                struct CronPayload {
                    expression: String,
                }
                let parsed =
                    serde_json::from_str::<CronPayload>(payload).map_err(|err| err.to_string())?;
                let schedule = CronSchedule::parse(&parsed.expression)?;
                Ok(Self::Cron {
                    expression: parsed.expression,
                    schedule,
                })
            }
            other => Err(format!("Unsupported trigger kind '{}'", other)),
        }
    }

    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::At { .. } => "at",
            Self::Interval { .. } => "interval",
            Self::Cron { .. } => "cron",
        }
    }

    pub(crate) fn label(&self) -> String {
        match self {
            Self::At { at_ms } => format!("at {}", at_ms),
            Self::Interval { every_ms, .. } => format!("every {}s", every_ms / 1_000),
            Self::Cron { expression, .. } => format!("cron {}", expression),
        }
    }

    pub(crate) fn to_payload_json(&self) -> Result<String, String> {
        match self {
            Self::At { at_ms } => serde_json::to_string(&serde_json::json!({ "at_ms": at_ms }))
                .map_err(|err| err.to_string()),
            Self::Interval {
                every_ms,
                anchor_ms,
            } => serde_json::to_string(&serde_json::json!({
                "every_ms": every_ms,
                "anchor_ms": anchor_ms,
            }))
            .map_err(|err| err.to_string()),
            Self::Cron { expression, .. } => serde_json::to_string(&serde_json::json!({
                "expression": expression,
            }))
            .map_err(|err| err.to_string()),
        }
    }

    pub(crate) fn next_after(&self, after_ms: i64) -> Option<i64> {
        match self {
            Self::At { at_ms } => (*at_ms > after_ms).then_some(*at_ms),
            Self::Interval {
                every_ms,
                anchor_ms,
            } => {
                if after_ms < *anchor_ms {
                    return Some(*anchor_ms);
                }
                let every_ms = *every_ms as i64;
                let delta = after_ms.saturating_sub(*anchor_ms);
                let ticks = delta.div_euclid(every_ms) + 1;
                Some(anchor_ms.saturating_add(ticks.saturating_mul(every_ms)))
            }
            Self::Cron { schedule, .. } => schedule.next_after(after_ms),
        }
    }
}

impl ScheduledJobState {
    pub(crate) fn from_str(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "retry_wait" => Self::RetryWait,
            "completed" => Self::Completed,
            "disabled" => Self::Disabled,
            _ => Self::Idle,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::RetryWait => "retry_wait",
            Self::Completed => "completed",
            Self::Disabled => "disabled",
        }
    }
}

impl CronSchedule {
    pub(crate) fn parse(expression: &str) -> Result<Self, String> {
        let parts = expression.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 5 {
            return Err("Cron expression must have 5 fields: min hour dom month dow".to_string());
        }
        Ok(Self {
            minute: CronField::parse(parts[0], 0, 59)?,
            hour: CronField::parse(parts[1], 0, 23)?,
            day_of_month: CronField::parse(parts[2], 1, 31)?,
            month: CronField::parse(parts[3], 1, 12)?,
            day_of_week: CronField::parse(parts[4], 0, 6)?,
        })
    }

    fn next_after(&self, after_ms: i64) -> Option<i64> {
        let next_minute_ms = after_ms.div_euclid(60_000).saturating_add(1) * 60_000;
        let mut candidate = DateTime::<Utc>::from_timestamp_millis(next_minute_ms)?;
        for _ in 0..(366 * 24 * 60) {
            if self.matches(candidate) {
                return Some(candidate.timestamp_millis());
            }
            candidate += ChronoDuration::minutes(1);
        }
        None
    }

    fn matches(&self, candidate: DateTime<Utc>) -> bool {
        self.minute.matches(candidate.minute())
            && self.hour.matches(candidate.hour())
            && self.day_of_month.matches(candidate.day())
            && self.month.matches(candidate.month())
            && self
                .day_of_week
                .matches(candidate.weekday().num_days_from_sunday())
    }
}

impl CronField {
    fn parse(input: &str, min: u32, max: u32) -> Result<Self, String> {
        let mut values = BTreeSet::new();
        for part in input.split(',') {
            let part = part.trim();
            if part.is_empty() {
                return Err("Cron field cannot be empty".to_string());
            }
            if let Some((base, step_raw)) = part.split_once('/') {
                let step = step_raw
                    .parse::<u32>()
                    .map_err(|_| format!("Invalid cron step '{}'", step_raw))?;
                if step == 0 {
                    return Err("Cron step must be > 0".to_string());
                }
                let (range_start, range_end) = parse_range(base, min, max)?;
                let mut current = range_start;
                while current <= range_end {
                    values.insert(current);
                    current = current.saturating_add(step);
                    if current == 0 {
                        break;
                    }
                }
                continue;
            }

            let (range_start, range_end) = parse_range(part, min, max)?;
            for value in range_start..=range_end {
                values.insert(value);
            }
        }

        if values.is_empty() {
            return Err("Cron field resolved to an empty set".to_string());
        }

        Ok(Self { values })
    }

    fn matches(&self, value: u32) -> bool {
        self.values.contains(&value)
    }
}

fn parse_range(input: &str, min: u32, max: u32) -> Result<(u32, u32), String> {
    if input == "*" {
        return Ok((min, max));
    }
    if let Some((start_raw, end_raw)) = input.split_once('-') {
        let start = parse_cron_number(start_raw, min, max)?;
        let end = parse_cron_number(end_raw, min, max)?;
        if start > end {
            return Err(format!("Invalid cron range '{}'", input));
        }
        return Ok((start, end));
    }
    let value = parse_cron_number(input, min, max)?;
    Ok((value, value))
}

fn parse_cron_number(raw: &str, min: u32, max: u32) -> Result<u32, String> {
    let value = raw
        .parse::<u32>()
        .map_err(|_| format!("Invalid cron value '{}'", raw))?;
    if value < min || value > max {
        return Err(format!(
            "Cron value '{}' out of range {}..={}",
            raw, min, max
        ));
    }
    Ok(value)
}
