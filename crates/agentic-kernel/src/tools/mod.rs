use std::fs;

use crate::config::kernel_config;
use crate::tool_registry::ToolRegistry;

pub mod api;
pub mod audit;
pub(crate) mod builtins;
pub mod dispatcher;
pub mod error;
pub mod executor;
pub mod governance;
pub mod invocation;
pub mod parser;
pub mod path_guard;
pub mod policy;
pub mod runner;
pub mod schema;
pub(crate) mod workspace_tools;

use path_guard::workspace_root;

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
    caller: invocation::ToolCaller,
    rate_map: &mut SyscallRateMap,
    registry: &ToolRegistry,
) -> SysCallOutcome {
    let clean_cmd = command_block.trim();
    let context = invocation::ToolContext {
        pid: Some(pid),
        session_id: None,
        caller,
        transport: invocation::ToolInvocationTransport::Text,
    };

    let invocation = match parser::parse_text_invocation(clean_cmd) {
        Ok(inv) => inv,
        Err(e) => {
            return SysCallOutcome {
                output: format!("SysCall Error: {}", e),
                success: false,
                duration_ms: 0,
                should_kill_process: false,
            };
        }
    };

    let result = governance::govern_tool_execution(&invocation, &context, registry, pid, rate_map);

    SysCallOutcome {
        output: result.output,
        success: result.success,
        duration_ms: result.duration_ms,
        should_kill_process: result.should_kill_process,
    }
}

pub(crate) fn validates_tool_invocation(command_block: &str) -> bool {
    parser::parse_text_invocation(command_block).is_ok()
}

#[cfg(test)]
mod macro_tests;
