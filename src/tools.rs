use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const WORKSPACE_DIR: &str = "./workspace";
const AUDIT_LOG_FILE: &str = "syscall_audit.log";
const OUTPUT_TRUNCATE_LEN: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SandboxMode {
    Host,
    Container,
    Wasm,
}

#[derive(Debug, Clone, Copy)]
struct SysCallConfig {
    mode: SandboxMode,
    allow_host_fallback: bool,
    timeout_s: u64,
    max_calls_per_window: usize,
    window_s: u64,
    error_burst_kill: usize,
}

#[derive(Debug, Clone)]
struct RateState {
    calls_in_window: VecDeque<Instant>,
    consecutive_errors: usize,
}

#[derive(Debug, Clone)]
pub struct SysCallOutcome {
    pub output: String,
    pub should_kill_process: bool,
}

static RATE_STATES: OnceLock<Mutex<HashMap<u64, RateState>>> = OnceLock::new();

fn rate_states() -> &'static Mutex<HashMap<u64, RateState>> {
    RATE_STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

fn syscall_config() -> SysCallConfig {
    let mode = match std::env::var("AGENTIC_SANDBOX_MODE")
        .unwrap_or_else(|_| "host".to_string())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "container" => SandboxMode::Container,
        "wasm" => SandboxMode::Wasm,
        _ => SandboxMode::Host,
    };

    SysCallConfig {
        mode,
        allow_host_fallback: env_bool("AGENTIC_ALLOW_HOST_FALLBACK", true),
        timeout_s: env_u64("AGENTIC_SYSCALL_TIMEOUT_S", 8),
        max_calls_per_window: env_usize("AGENTIC_SYSCALL_MAX_PER_WINDOW", 12),
        window_s: env_u64("AGENTIC_SYSCALL_WINDOW_S", 10),
        error_burst_kill: env_usize("AGENTIC_SYSCALL_ERROR_BURST_KILL", 3),
    }
}

fn workspace_root() -> Result<PathBuf, String> {
    fs::create_dir_all(WORKSPACE_DIR)
        .map_err(|e| format!("SysCall Error: Failed to create workspace: {}", e))?;

    fs::canonicalize(WORKSPACE_DIR)
        .map_err(|e| format!("SysCall Error: Failed to resolve workspace root: {}", e))
}

fn normalize_relative_path(root: &Path, input: &str) -> Result<PathBuf, String> {
    let clean_input = input.trim();
    if clean_input.is_empty() {
        return Err("SysCall Error: Empty filename.".to_string());
    }
    if clean_input.contains('\0') {
        return Err("SysCall Error: Invalid filename (contains NUL).".to_string());
    }

    let candidate = Path::new(clean_input);
    if candidate.is_absolute() {
        return Err("SysCall Error: Absolute paths are not allowed.".to_string());
    }

    let mut out = root.to_path_buf();
    for comp in candidate.components() {
        match comp {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            Component::ParentDir => {
                if out == root {
                    return Err("SysCall Error: Path traversal denied.".to_string());
                }
                out.pop();
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("SysCall Error: Invalid path root/prefix.".to_string());
            }
        }
    }

    if !out.starts_with(root) {
        return Err("SysCall Error: Path escapes workspace.".to_string());
    }

    Ok(out)
}

fn resolve_safe_path(filename: &str) -> Result<PathBuf, String> {
    let root = workspace_root()?;
    normalize_relative_path(&root, filename)
}

fn truncate_output(text: &str) -> String {
    if text.len() > OUTPUT_TRUNCATE_LEN {
        format!("{}... (Output Truncated)", &text[..OUTPUT_TRUNCATE_LEN])
    } else if text.trim().is_empty() {
        "Done (No Output)".to_string()
    } else {
        text.to_string()
    }
}

fn run_with_timeout(
    cwd: &Path,
    program: &str,
    args: &[String],
    timeout_s: u64,
) -> Result<std::process::Output, String> {
    let mut wrapped = Command::new("timeout");
    wrapped
        .arg("--signal=KILL")
        .arg(format!("{}s", timeout_s.max(1)))
        .arg(program);
    for arg in args {
        wrapped.arg(arg);
    }
    wrapped.current_dir(cwd).output().map_err(|e| {
        format!(
            "SysCall Error: Failed to execute '{}' via timeout wrapper: {}",
            program, e
        )
    })
}

fn run_host_python(script_path: &Path, timeout_s: u64) -> Result<String, String> {
    let cwd = workspace_root()?;
    let script_name = script_path
        .file_name()
        .ok_or_else(|| "SysCall Error: Invalid script filename.".to_string())?
        .to_string_lossy()
        .to_string();

    let output = run_with_timeout(&cwd, "python3", &[script_name], timeout_s)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.code() == Some(124) || output.status.code() == Some(137) {
        return Err(format!(
            "SysCall Error: Python execution timed out after {}s.",
            timeout_s.max(1)
        ));
    }

    if output.status.success() {
        if stderr.trim().is_empty() {
            Ok(truncate_output(&stdout))
        } else {
            Ok(truncate_output(&format!("Output:\n{}\nErrors:\n{}", stdout, stderr)))
        }
    } else {
        Err(truncate_output(&format!(
            "SysCall Error: Python failed (status={:?}).\n{}{}",
            output.status.code(),
            if stdout.is_empty() { "" } else { &format!("stdout:\n{}\n", stdout) },
            if stderr.is_empty() { "" } else { &format!("stderr:\n{}", stderr) }
        )))
    }
}

fn run_container_python(script_path: &Path, timeout_s: u64) -> Result<String, String> {
    let cwd = workspace_root()?;
    let script_name = script_path
        .file_name()
        .ok_or_else(|| "SysCall Error: Invalid script filename.".to_string())?
        .to_string_lossy()
        .to_string();

    let volume = format!("{}:/workspace", cwd.display());
    let args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--network".to_string(),
        "none".to_string(),
        "-m".to_string(),
        "256m".to_string(),
        "--cpus".to_string(),
        "1".to_string(),
        "-v".to_string(),
        volume,
        "-w".to_string(),
        "/workspace".to_string(),
        "python:3.11-alpine".to_string(),
        "python3".to_string(),
        script_name,
    ];

    let output = run_with_timeout(&cwd, "docker", &args, timeout_s)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.code() == Some(124) || output.status.code() == Some(137) {
        return Err(format!(
            "SysCall Error: Container execution timed out after {}s.",
            timeout_s.max(1)
        ));
    }

    if output.status.success() {
        Ok(truncate_output(&stdout))
    } else {
        Err(truncate_output(&format!(
            "SysCall Error: Container runner failed (status={:?}).\n{}{}",
            output.status.code(),
            if stdout.is_empty() { "" } else { &format!("stdout:\n{}\n", stdout) },
            if stderr.is_empty() { "" } else { &format!("stderr:\n{}", stderr) }
        )))
    }
}

fn execute_python_with_policy(code: &str, pid: u64, cfg: SysCallConfig) -> Result<String, String> {
    let clean_code = code
        .trim()
        .trim_start_matches("```python")
        .trim_start_matches("```")
        .trim_end_matches("```");

    let root = workspace_root()?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis();
    let temp_filename = format!("agent_script_{}_{}.py", pid, ts);
    let script_path = root.join(temp_filename);

    fs::write(&script_path, clean_code)
        .map_err(|e| format!("SysCall Error: Failed to write temp file: {}", e))?;

    let run_result = match cfg.mode {
        SandboxMode::Host => run_host_python(&script_path, cfg.timeout_s),
        SandboxMode::Container => match run_container_python(&script_path, cfg.timeout_s) {
            Ok(out) => Ok(out),
            Err(e) if cfg.allow_host_fallback => {
                let host_out = run_host_python(&script_path, cfg.timeout_s)?;
                Ok(format!(
                    "[Sandbox fallback: container->host due to error]\n{}\n{}",
                    e, host_out
                ))
            }
            Err(e) => Err(e),
        },
        SandboxMode::Wasm => {
            if cfg.allow_host_fallback {
                let host_out = run_host_python(&script_path, cfg.timeout_s)?;
                Ok(format!(
                    "[Sandbox fallback: wasm->host (wasm runner not configured)]\n{}",
                    host_out
                ))
            } else {
                Err("SysCall Error: Sandbox mode 'wasm' selected but no wasm runner configured and host fallback disabled.".to_string())
            }
        }
    };

    let _ = fs::remove_file(&script_path);
    run_result
}

fn handle_write_file(args: &str) -> Result<String, String> {
    let parts: Vec<&str> = args.splitn(2, '|').collect();
    if parts.len() < 2 {
        return Err("SysCall Error: Usage [[WRITE_FILE: filename | content]]".to_string());
    }

    let filename = parts[0].trim();
    let content = parts[1].trim_start();
    let path = resolve_safe_path(filename)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("SysCall Error: Failed to create parent dir: {}", e))?;
    }

    fs::write(&path, content).map_err(|e| format!("SysCall Error: Write failed: {}", e))?;
    Ok(format!(
        "Success: File '{}' written ({} bytes).",
        filename,
        content.len()
    ))
}

fn handle_read_file(filename: &str) -> Result<String, String> {
    let path = resolve_safe_path(filename)?;
    let meta = fs::metadata(&path).map_err(|e| format!("SysCall Error: Read failed: {}", e))?;
    if meta.len() > 1024 * 1024 {
        return Err("SysCall Error: Refusing to read files larger than 1MB.".to_string());
    }
    fs::read_to_string(&path).map_err(|e| format!("SysCall Error: Read failed: {}", e))
}

fn handle_list_files() -> Result<String, String> {
    let root = workspace_root()?;
    match fs::read_dir(root) {
        Ok(entries) => {
            let files: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            if files.is_empty() {
                Ok("Workspace is empty.".to_string())
            } else {
                Ok(format!("Files:\n- {}", files.join("\n- ")))
            }
        }
        Err(e) => Err(format!("SysCall Error: LS failed: {}", e)),
    }
}

fn sanitize_log_value(value: &str) -> String {
    value.replace('\n', "\\n").replace('\r', "")
}

fn append_audit_log(
    pid: u64,
    mode: SandboxMode,
    command: &str,
    success: bool,
    duration_ms: u128,
    should_kill: bool,
    detail: &str,
) {
    let root = match workspace_root() {
        Ok(path) => path,
        Err(_) => return,
    };

    let log_path = root.join(AUDIT_LOG_FILE);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis();
    let line = format!(
        "ts_ms={} pid={} mode={:?} success={} kill={} duration_ms={} cmd=\"{}\" detail=\"{}\"\n",
        ts,
        pid,
        mode,
        success,
        should_kill,
        duration_ms,
        sanitize_log_value(command),
        sanitize_log_value(detail),
    );

    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

fn rate_limit_precheck(pid: u64, cfg: SysCallConfig) -> Result<(), String> {
    let mut lock = rate_states().lock().unwrap();
    let now = Instant::now();
    let state = lock.entry(pid).or_insert_with(|| RateState {
        calls_in_window: VecDeque::new(),
        consecutive_errors: 0,
    });

    let max_age = Duration::from_secs(cfg.window_s.max(1));
    while let Some(front) = state.calls_in_window.front().copied() {
        if now.duration_since(front) > max_age {
            state.calls_in_window.pop_front();
        } else {
            break;
        }
    }

    if state.calls_in_window.len() >= cfg.max_calls_per_window.max(1) {
        return Err(format!(
            "SysCall Error: Rate limit exceeded (>{} calls in {}s).",
            cfg.max_calls_per_window.max(1),
            cfg.window_s.max(1)
        ));
    }

    state.calls_in_window.push_back(now);
    Ok(())
}

fn rate_limit_postcheck(pid: u64, success: bool, cfg: SysCallConfig) -> bool {
    let mut lock = rate_states().lock().unwrap();
    let state = lock.entry(pid).or_insert_with(|| RateState {
        calls_in_window: VecDeque::new(),
        consecutive_errors: 0,
    });

    if success {
        state.consecutive_errors = 0;
        false
    } else {
        state.consecutive_errors += 1;
        state.consecutive_errors >= cfg.error_burst_kill.max(1)
    }
}

pub fn handle_syscall(command_block: &str, pid: u64) -> SysCallOutcome {
    let cfg = syscall_config();
    let start = Instant::now();
    let clean_cmd = command_block.trim();

    if let Err(e) = rate_limit_precheck(pid, cfg) {
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

    let kill_from_burst = rate_limit_postcheck(pid, success, cfg);
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
pub fn reset_syscall_state_for_tests() {
    if let Ok(mut lock) = rate_states().lock() {
        lock.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{handle_syscall, reset_syscall_state_for_tests};
    use std::sync::{Mutex, OnceLock};

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn denies_path_traversal() {
        let _guard = test_lock().lock().unwrap();
        reset_syscall_state_for_tests();

        let out = handle_syscall("READ_FILE: ../secret.txt", 10);
        assert!(out.output.contains("Path traversal") || out.output.contains("escapes workspace"));
    }

    #[test]
    fn rate_limit_can_kill_process() {
        let _guard = test_lock().lock().unwrap();
        reset_syscall_state_for_tests();
        std::env::set_var("AGENTIC_SYSCALL_MAX_PER_WINDOW", "1");
        std::env::set_var("AGENTIC_SYSCALL_WINDOW_S", "60");

        let _ = handle_syscall("LS", 22);
        let second = handle_syscall("LS", 22);
        assert!(second.should_kill_process);
        assert!(second.output.contains("Rate limit exceeded"));

        std::env::remove_var("AGENTIC_SYSCALL_MAX_PER_WINDOW");
        std::env::remove_var("AGENTIC_SYSCALL_WINDOW_S");
    }

    #[test]
    fn disable_host_fallback_rejects_unavailable_wasm_runner() {
        let _guard = test_lock().lock().unwrap();
        reset_syscall_state_for_tests();
        std::env::set_var("AGENTIC_SANDBOX_MODE", "wasm");
        std::env::set_var("AGENTIC_ALLOW_HOST_FALLBACK", "false");

        let out = handle_syscall("PYTHON: print('x')", 31);
        assert!(out.output.contains("wasm") || out.output.contains("fallback disabled"));

        std::env::remove_var("AGENTIC_SANDBOX_MODE");
        std::env::remove_var("AGENTIC_ALLOW_HOST_FALLBACK");
    }
}
