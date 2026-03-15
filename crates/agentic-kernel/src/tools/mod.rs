use std::fs;
use std::time::Instant;

use crate::config::kernel_config;
use crate::tool_registry::ToolRegistry;

pub mod api;
pub mod audit;
pub mod dispatcher;
pub mod error;
pub mod executor;
pub mod invocation;
pub mod parser;
pub mod path_guard;
pub mod policy;
pub mod runner;
pub mod schema;

use audit::{append_audit_log, ToolAuditRecord};
use path_guard::workspace_root;
use policy::{rate_limit_postcheck, rate_limit_precheck, syscall_config};

pub(crate) use policy::SyscallRateMap;

/// Remove stale `agent_script_*.py` temp files left by previous crashes.
/// Called once at kernel boot.
pub fn cleanup_stale_temp_scripts() {
    let root = match workspace_root() {
        Ok(p) => p,
        Err(_) => return,
    };
    let prefix = &kernel_config().tools.temp_script_prefix;
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(prefix) && name.ends_with(".py") {
            if let Err(e) = fs::remove_file(entry.path()) {
                tracing::warn!(file = %name, %e, "failed to remove stale temp script");
            } else {
                tracing::debug!(file = %name, "removed stale temp script");
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct SysCallOutcome {
    pub output: String,
    pub success: bool,
    pub duration_ms: u128,
    pub should_kill_process: bool,
}

pub fn handle_syscall(
    command_block: &str,
    pid: u64,
    rate_map: &mut SyscallRateMap,
    registry: &ToolRegistry,
) -> SysCallOutcome {
    let cfg = syscall_config();
    let start = Instant::now();
    let clean_cmd = command_block.trim();

    if let Err(e) = rate_limit_precheck(pid, cfg, rate_map) {
        let err = error::ToolError::RateLimited(e);
        append_audit_log(ToolAuditRecord {
            pid,
            mode: cfg.mode,
            command: clean_cmd,
            success: false,
            duration_ms: start.elapsed().as_millis(),
            should_kill: true,
            detail: &err.to_string(),
            context: &invocation::ToolContext {
                pid: Some(pid),
                session_id: None,
                caller: invocation::ToolCaller::AgentText,
                transport: invocation::ToolInvocationTransport::Text,
            },
            tool_name: None,
        });
        return SysCallOutcome {
            output: err.to_string(),
            success: false,
            duration_ms: start.elapsed().as_millis(),
            should_kill_process: true,
        };
    }

    let context = invocation::ToolContext {
        pid: Some(pid),
        session_id: None,
        caller: invocation::ToolCaller::AgentText,
        transport: invocation::ToolInvocationTransport::Text,
    };

    let exec_result = executor::execute_text_invocation(clean_cmd, &context, registry);

    let (success, output, tool_name) = match exec_result {
        Ok(execution) => {
            let rendered = if let Some(text) = execution.result.display_text {
                text
            } else {
                serde_json::to_string_pretty(&execution.result.output)
                    .unwrap_or_else(|_| "{}".into())
            };
            (true, rendered, Some(execution.invocation.name))
        }
        Err(e) => (false, format!("SysCall Error: {}", e), None),
    };

    let kill_from_burst = rate_limit_postcheck(pid, success, cfg, rate_map);
    let mut final_output = output;
    if kill_from_burst {
        final_output.push_str("\nSysCall Guard: process killed due to repeated syscall failures.");
    }

    append_audit_log(ToolAuditRecord {
        pid,
        mode: cfg.mode,
        command: clean_cmd,
        success,
        duration_ms: start.elapsed().as_millis(),
        should_kill: kill_from_burst,
        detail: &final_output,
        context: &context,
        tool_name: tool_name.as_deref(),
    });

    SysCallOutcome {
        output: final_output,
        success,
        duration_ms: start.elapsed().as_millis(),
        should_kill_process: kill_from_burst,
    }
}

pub(crate) fn validates_tool_invocation(command_block: &str) -> bool {
    parser::parse_text_invocation(command_block).is_ok()
}
