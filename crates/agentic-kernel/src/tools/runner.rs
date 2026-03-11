use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::kernel_config;

use super::path_guard::{resolve_safe_path, workspace_root};
use super::policy::{SandboxMode, SysCallConfig};

fn truncate_output(text: &str) -> String {
    let limit = kernel_config().tools.output_truncate_len;
    if text.len() > limit {
        let mut end = limit;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}... (Output Truncated)", &text[..end])
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
            Ok(truncate_output(&format!(
                "Output:\n{}\nErrors:\n{}",
                stdout, stderr
            )))
        }
    } else {
        Err(truncate_output(&format!(
            "SysCall Error: Python failed (status={:?}).\n{}{}",
            output.status.code(),
            if stdout.is_empty() {
                ""
            } else {
                &format!("stdout:\n{}\n", stdout)
            },
            if stderr.is_empty() {
                ""
            } else {
                &format!("stderr:\n{}", stderr)
            }
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
            if stdout.is_empty() {
                ""
            } else {
                &format!("stdout:\n{}\n", stdout)
            },
            if stderr.is_empty() {
                ""
            } else {
                &format!("stderr:\n{}", stderr)
            }
        )))
    }
}

pub(crate) fn execute_python_with_policy(
    code: &str,
    pid: u64,
    cfg: SysCallConfig,
) -> Result<String, String> {
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

pub(crate) fn handle_write_file(args: &str) -> Result<String, String> {
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

pub(crate) fn handle_read_file(filename: &str) -> Result<String, String> {
    let path = resolve_safe_path(filename)?;
    let meta = fs::metadata(&path).map_err(|e| format!("SysCall Error: Read failed: {}", e))?;
    if meta.len() > 1024 * 1024 {
        return Err("SysCall Error: Refusing to read files larger than 1MB.".to_string());
    }
    fs::read_to_string(&path).map_err(|e| format!("SysCall Error: Read failed: {}", e))
}

pub(crate) fn handle_list_files() -> Result<String, String> {
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

#[cfg(test)]
mod tests {
    use super::truncate_output;
    use crate::config::kernel_config;

    #[test]
    fn truncate_output_preserves_utf8_boundaries() {
        let limit = kernel_config().tools.output_truncate_len;
        let text = format!("{}😀", "a".repeat(limit));
        let truncated = truncate_output(&text);
        assert!(truncated.starts_with(&"a".repeat(limit)));
        assert!(truncated.ends_with("... (Output Truncated)"));
        assert!(!truncated.contains('😀'));
    }
}
