use std::time::Instant;

use crate::tool_registry::ToolRegistry;

use super::audit::{append_audit_log, ToolAuditRecord};
use super::dispatcher::ToolDispatcher;
use super::error::ToolError;
use super::invocation::{ToolContext, ToolInvocation};
use super::policy::{rate_limit_postcheck, rate_limit_precheck, syscall_config, SysCallConfig};
use super::SyscallRateMap;

/// Outcome of a governed tool execution.
///
/// Contains the rendered output and governance metadata produced by
/// rate-limiting, audit, and policy enforcement.
#[derive(Debug, Clone)]
pub struct GovernedToolResult {
    pub output: String,
    pub success: bool,
    pub duration_ms: u128,
    pub should_kill_process: bool,
}

/// Execute a tool invocation through the full governance pipeline:
///   1. Rate-limit precheck
///   2. Dispatch via `ToolDispatcher`
///   3. Rate-limit postcheck (burst detection)
///   4. Audit log
///
/// Both the text path (`handle_syscall`) and the structured path converge here
/// so that audit, rate-limiting, and kill behaviour are identical regardless of
/// transport.
pub fn govern_tool_execution(
    invocation: &ToolInvocation,
    context: &ToolContext,
    registry: &ToolRegistry,
    pid: u64,
    rate_map: &mut SyscallRateMap,
) -> GovernedToolResult {
    let cfg = syscall_config();
    let start = Instant::now();

    // — Rate-limit precheck —
    if let Err(e) = rate_limit_precheck(pid, cfg, rate_map) {
        let err = ToolError::RateLimited(e);
        append_governed_audit(
            pid,
            cfg,
            invocation,
            &err.to_string(),
            false,
            true,
            start,
            context,
            None,
        );
        return GovernedToolResult {
            output: err.to_string(),
            success: false,
            duration_ms: start.elapsed().as_millis(),
            should_kill_process: true,
        };
    }

    // — Dispatch —
    let dispatcher = ToolDispatcher::new();
    let exec_result = dispatcher.dispatch(invocation, context, registry);

    let (success, output, tool_name) = match exec_result {
        Ok(result) => {
            let rendered = if let Some(text) = result.display_text {
                text
            } else {
                serde_json::to_string_pretty(&result.output).unwrap_or_else(|_| "{}".into())
            };
            (true, rendered, Some(invocation.name.clone()))
        }
        Err(e) => (false, format!("SysCall Error: {}", e), None),
    };

    // — Rate-limit postcheck (burst kill) —
    let kill_from_burst = rate_limit_postcheck(pid, success, cfg, rate_map);
    let mut final_output = output;
    if kill_from_burst {
        final_output.push_str("\nSysCall Guard: process killed due to repeated syscall failures.");
    }

    // — Audit —
    append_governed_audit(
        pid,
        cfg,
        invocation,
        &final_output,
        success,
        kill_from_burst,
        start,
        context,
        tool_name.as_deref(),
    );

    GovernedToolResult {
        output: final_output,
        success,
        duration_ms: start.elapsed().as_millis(),
        should_kill_process: kill_from_burst,
    }
}

#[allow(clippy::too_many_arguments)]
fn append_governed_audit(
    pid: u64,
    cfg: SysCallConfig,
    invocation: &ToolInvocation,
    detail: &str,
    success: bool,
    should_kill: bool,
    start: Instant,
    context: &ToolContext,
    tool_name: Option<&str>,
) {
    let command = format!(
        "TOOL:{} {}",
        invocation.name,
        serde_json::to_string(&invocation.input).unwrap_or_else(|_| "{}".to_string())
    );
    append_audit_log(ToolAuditRecord {
        pid,
        mode: cfg.mode,
        command: &command,
        success,
        duration_ms: start.elapsed().as_millis(),
        should_kill,
        detail,
        context,
        tool_call_id: invocation.call_id.as_deref(),
        tool_name,
    });
}
