use std::fs;
use std::time::Instant;

use crate::config::kernel_config;
use crate::tool_registry::ToolRegistry;

pub mod api;
pub mod audit;
pub mod dispatcher;
pub mod error;
pub mod invocation;
pub mod parser;
pub mod path_guard;
pub mod policy;
pub mod runner;
pub mod schema;

use audit::append_audit_log;
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
        append_audit_log(
            pid,
            cfg.mode,
            clean_cmd,
            false,
            start.elapsed().as_millis(),
            true,
            &err.to_string(),
        );
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
    };

    let exec_result = parser::parse_text_invocation(clean_cmd).and_then(|inv| {
        let dispatcher = dispatcher::ToolDispatcher::new();
        dispatcher.dispatch(&inv, &context, registry)
    });

    let (success, output) = match exec_result {
        Ok(res) => {
            if let Some(text) = res.display_text {
                (true, text)
            } else {
                (
                    true,
                    serde_json::to_string_pretty(&res.output).unwrap_or_else(|_| "{}".into()),
                )
            }
        }
        Err(e) => (false, format!("SysCall Error: {}", e)),
    };

    let kill_from_burst = rate_limit_postcheck(pid, success, cfg, rate_map);
    let mut final_output = output;
    if kill_from_burst {
        final_output.push_str("\nSysCall Guard: process killed due to repeated syscall failures.");
    }

    append_audit_log(
        pid,
        cfg.mode,
        clean_cmd,
        success,
        start.elapsed().as_millis(),
        kill_from_burst,
        &final_output,
    );

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
