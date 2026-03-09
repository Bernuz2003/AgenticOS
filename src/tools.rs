use std::fs;
use std::time::Instant;

use crate::config::kernel_config;

mod audit;
mod path_guard;
mod policy;
mod runner;

use audit::append_audit_log;
use path_guard::workspace_root;
use policy::{rate_limit_postcheck, rate_limit_precheck, syscall_config};
use runner::{execute_python_with_policy, handle_list_files, handle_read_file, handle_write_file};

pub(crate) use policy::SyscallRateMap;

/// Remove stale `agent_script_*.py` temp files left by previous crashes.
/// Called once at kernel boot.
pub fn cleanup_stale_temp_scripts() {
    let root = match workspace_root() {
        Ok(p) => p,
        Err(_) => return,
    };
    let prefix = &kernel_config().tools.temp_script_prefix;
    let Ok(entries) = fs::read_dir(&root) else { return };
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
    pub should_kill_process: bool,
}

pub fn handle_syscall(command_block: &str, pid: u64, rate_map: &mut SyscallRateMap) -> SysCallOutcome {
    let cfg = syscall_config();
    let start = Instant::now();
    let clean_cmd = command_block.trim();

    if let Err(e) = rate_limit_precheck(pid, cfg, rate_map) {
        append_audit_log(
            pid,
            cfg.mode,
            clean_cmd,
            false,
            start.elapsed().as_millis(),
            true,
            &e,
        );
        return SysCallOutcome {
            output: e,
            should_kill_process: true,
        };
    }

    let exec_result: Result<String, String> = if clean_cmd.starts_with("PYTHON:") {
        execute_python_with_policy(clean_cmd.trim_start_matches("PYTHON:"), pid, cfg)
    } else if clean_cmd.starts_with("WRITE_FILE:") {
        handle_write_file(clean_cmd.trim_start_matches("WRITE_FILE:"))
    } else if clean_cmd.starts_with("READ_FILE:") {
        handle_read_file(clean_cmd.trim_start_matches("READ_FILE:").trim())
    } else if clean_cmd == "LS" || clean_cmd.starts_with("LS ") {
        handle_list_files()
    } else if clean_cmd.starts_with("CALC:") {
        let expr = clean_cmd.trim_start_matches("CALC:").trim();
        execute_python_with_policy(&format!("print({})", expr), pid, cfg)
    } else {
        Err("SysCall Error: Unknown Tool or forbidden command.".to_string())
    };

    let (success, output) = match exec_result {
        Ok(msg) => (true, msg),
        Err(err) => (false, err),
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
        should_kill_process: kill_from_burst,
    }
}

#[cfg(test)]
mod tests {
    use super::{handle_syscall, SyscallRateMap};
    use std::sync::{Mutex, OnceLock};

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn denies_path_traversal() {
        let _guard = test_lock().lock().unwrap();
        let mut rate_map = SyscallRateMap::new();

        let out = handle_syscall("READ_FILE: ../secret.txt", 10, &mut rate_map);
        assert!(out.output.contains("Path traversal") || out.output.contains("escapes workspace"));
    }

    #[test]
    fn rate_limit_can_kill_process() {
        let _guard = test_lock().lock().unwrap();
        let mut rate_map = SyscallRateMap::new();
        std::env::set_var("AGENTIC_SYSCALL_MAX_PER_WINDOW", "1");
        std::env::set_var("AGENTIC_SYSCALL_WINDOW_S", "60");

        let _ = handle_syscall("LS", 22, &mut rate_map);
        let second = handle_syscall("LS", 22, &mut rate_map);
        assert!(second.should_kill_process);
        assert!(second.output.contains("Rate limit exceeded"));

        std::env::remove_var("AGENTIC_SYSCALL_MAX_PER_WINDOW");
        std::env::remove_var("AGENTIC_SYSCALL_WINDOW_S");
    }

    #[test]
    fn disable_host_fallback_rejects_unavailable_wasm_runner() {
        let _guard = test_lock().lock().unwrap();
        let mut rate_map = SyscallRateMap::new();
        std::env::set_var("AGENTIC_SANDBOX_MODE", "wasm");
        std::env::set_var("AGENTIC_ALLOW_HOST_FALLBACK", "false");

        let out = handle_syscall("PYTHON: print('x')", 31, &mut rate_map);
        assert!(out.output.contains("wasm") || out.output.contains("fallback disabled"));

        std::env::remove_var("AGENTIC_SANDBOX_MODE");
        std::env::remove_var("AGENTIC_ALLOW_HOST_FALLBACK");
    }
}
