use serde_json::Value;

use super::checkpoints::{parse_json_array_strings, parse_json_array_values};
use super::models::CoreDumpToolInvocation;
use crate::config::kernel_config;
use crate::storage::StorageService;

pub(crate) fn load_manifest_tool_invocation_history(
    storage: &StorageService,
    pid: u64,
) -> Result<Vec<CoreDumpToolInvocation>, String> {
    storage
        .recent_tool_invocations_for_pid(
            pid,
            kernel_config().core_dump.max_tool_invocations_per_pid,
        )
        .map_err(|err| err.to_string())?
        .into_iter()
        .rev()
        .map(|record| {
            let input = serde_json::from_str::<Value>(&record.input_json)
                .map_err(|err| format!("parse tool input '{}': {err}", record.tool_call_id))?;
            let output = record
                .output_json
                .as_deref()
                .map(serde_json::from_str::<Value>)
                .transpose()
                .map_err(|err| format!("parse tool output '{}': {err}", record.tool_call_id))?;
            let warnings = parse_json_array_strings(record.warnings_json.as_deref())?;
            let effects = parse_json_array_values(record.effect_json.as_deref())?;
            Ok(CoreDumpToolInvocation {
                tool_call_id: record.tool_call_id,
                recorded_at_ms: record.recorded_at_ms,
                updated_at_ms: record.updated_at_ms,
                session_id: record.session_id,
                pid: record.pid,
                runtime_id: record.runtime_id,
                tool_name: record.tool_name,
                caller: record.caller,
                transport: record.transport,
                status: record.status,
                command_text: record.command_text,
                input,
                output,
                output_text: record.output_text,
                warnings,
                error_kind: record.error_kind,
                error_text: record.error_text,
                effects,
                duration_ms: record.duration_ms,
                kill: record.kill,
            })
        })
        .collect()
}
