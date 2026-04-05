use agentic_kernel_macros::agentic_tool;
use chrono::{Local, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::error::ToolError;
use super::invocation::ToolContext;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
struct GetTimeInput {}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct GetTimeOutput {
    output: String,
    unix_timestamp_ms: i64,
    datetime_local: String,
    datetime_utc: String,
    date: String,
    time: String,
    weekday: String,
    timezone_offset: String,
}

#[agentic_tool(
    name = "get_time",
    description = "Return the current system time with local and UTC representations.",
    input_example = serde_json::json!({}),
    capabilities = ["time"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn get_time(_input: GetTimeInput, _ctx: &ToolContext) -> Result<GetTimeOutput, ToolError> {
    let local_now = Local::now();
    let utc_now = Utc::now();
    let output = format!(
        "Local time: {} ({})",
        local_now.to_rfc3339(),
        local_now.format("%A")
    );

    Ok(GetTimeOutput {
        output,
        unix_timestamp_ms: utc_now.timestamp_millis(),
        datetime_local: local_now.to_rfc3339(),
        datetime_utc: utc_now.to_rfc3339(),
        date: local_now.format("%Y-%m-%d").to_string(),
        time: local_now.format("%H:%M:%S").to_string(),
        weekday: local_now.format("%A").to_string(),
        timezone_offset: local_now.format("%:z").to_string(),
    })
}

#[cfg(test)]
#[path = "tests/system.rs"]
mod tests;
