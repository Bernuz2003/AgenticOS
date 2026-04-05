use std::fs;

use crate::config::kernel_config;
use crate::tool_registry::ToolRegistry;

pub mod api;
pub mod audit;
pub(crate) mod builtins;
pub(crate) mod command_tools;
pub mod dispatcher;
pub(crate) mod effects;
pub mod error;
pub mod executor;
pub mod governance;
pub(crate) mod host_exec;
pub(crate) mod human_tools;
pub mod invocation;
pub(crate) mod network_tools;
pub mod parser;
pub mod path_guard;
pub mod policy;
pub mod runner;
pub mod schema;
pub(crate) mod system_tools;
pub(crate) mod workspace_edit_tools;
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
    pub output_json: Option<serde_json::Value>,
    pub warnings: Vec<String>,
    pub error_kind: Option<String>,
    pub effects: Vec<serde_json::Value>,
}

pub fn handle_syscall(
    command_block: &str,
    pid: u64,
    caller: invocation::ToolCaller,
    permissions: invocation::ProcessPermissionPolicy,
    call_id: Option<String>,
    rate_map: &mut SyscallRateMap,
    registry: &ToolRegistry,
) -> SysCallOutcome {
    let clean_cmd = command_block.trim();
    let context = invocation::ToolContext {
        pid: Some(pid),
        session_id: None,
        caller,
        permissions,
        transport: invocation::ToolInvocationTransport::Text,
        call_id,
    };

    let invocation = match parser::parse_text_invocation(clean_cmd) {
        Ok(inv) => inv,
        Err(e) => {
            return SysCallOutcome {
                output: format!("SysCall Error: {}", e),
                success: false,
                duration_ms: 0,
                should_kill_process: false,
                output_json: None,
                warnings: Vec::new(),
                error_kind: Some("malformed_invocation".to_string()),
                effects: Vec::new(),
            };
        }
    };

    let result = governance::govern_tool_execution(&invocation, &context, registry, pid, rate_map);

    SysCallOutcome {
        output: result.output,
        success: result.success,
        duration_ms: result.duration_ms,
        should_kill_process: result.should_kill_process,
        output_json: result.output_json,
        warnings: result.warnings,
        error_kind: result.error_kind,
        effects: result.effects,
    }
}
#[cfg(test)]
#[path = "tests/macros.rs"]
mod macro_tests;
