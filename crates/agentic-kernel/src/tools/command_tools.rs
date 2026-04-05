use std::path::Path;
use std::time::Instant;

use agentic_kernel_macros::agentic_tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::kernel_config;

use super::error::ToolError;
use super::host_exec::run_with_timeout;
use super::invocation::ToolContext;
use super::workspace_tools::resolve_search_root;

const INTERACTIVE_PROGRAMS: &[&str] = &[
    "nano", "vim", "vi", "view", "less", "more", "top", "htop", "tmux", "screen",
];
const SHELL_PROGRAMS: &[&str] = &["sh", "bash", "zsh", "fish"];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExecCommandInput {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct ExecCommandOutput {
    output: String,
    program: String,
    args: Vec<String>,
    cwd: String,
    exit_code: i32,
    successful: bool,
    stdout: String,
    stderr: String,
    duration_ms: u64,
    truncated: bool,
}

#[agentic_tool(
    name = "exec_command",
    description = "Execute a non-interactive command inside the process-scoped workspace with timeout and captured stdout/stderr.",
    input_example = serde_json::json!({"program": "sh", "args": ["-lc", "pwd"], "cwd": ".", "timeout_ms": 5000}),
    capabilities = ["shell", "process", "exec"],
    dangerous = true,
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn exec_command(
    input: ExecCommandInput,
    ctx: &ToolContext,
) -> Result<ExecCommandOutput, ToolError> {
    let program = input.program.trim();
    if program.is_empty() {
        return Err(ToolError::InvalidInput(
            "exec_command".into(),
            "field 'program' cannot be empty".into(),
        ));
    }
    validate_command_request(program, &input.args)?;

    let cwd_root = resolve_search_root("exec_command", input.cwd.as_deref(), ctx)?;
    if !cwd_root.absolute.is_dir() {
        return Err(ToolError::InvalidInput(
            "exec_command".into(),
            format!("cwd '{}' is not a directory", cwd_root.display),
        ));
    }

    let timeout_ms = input.timeout_ms.unwrap_or_else(default_timeout_ms).max(1);
    let timeout_s = timeout_ms.div_ceil(1000);
    let start = Instant::now();
    let result = run_with_timeout(&cwd_root.absolute, program, &input.args, timeout_s)
        .map_err(|err| classify_command_failure("exec_command", &err, timeout_ms))?;
    let duration_ms = start.elapsed().as_millis() as u64;

    if matches!(result.status.code(), Some(124 | 137)) {
        return Err(ToolError::Timeout("exec_command".into(), timeout_ms));
    }

    let stdout = String::from_utf8_lossy(&result.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&result.stderr).into_owned();
    let (stdout, stdout_truncated) = truncate_stream(&stdout);
    let (stderr, stderr_truncated) = truncate_stream(&stderr);
    let truncated = stdout_truncated || stderr_truncated;
    let exit_code = result.status.code().unwrap_or(-1);
    let successful = result.status.success();
    let output = render_command_output(
        program,
        &input.args,
        exit_code,
        duration_ms,
        &stdout,
        &stderr,
        truncated,
    );

    Ok(ExecCommandOutput {
        output,
        program: program.to_string(),
        args: input.args,
        cwd: cwd_root.display,
        exit_code,
        successful,
        stdout,
        stderr,
        duration_ms,
        truncated,
    })
}

fn validate_command_request(program: &str, args: &[String]) -> Result<(), ToolError> {
    let program_name = basename(program);
    if INTERACTIVE_PROGRAMS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(program_name))
    {
        return Err(ToolError::InvalidInput(
            "exec_command".into(),
            format!("interactive program '{}' is not allowed", program_name),
        ));
    }

    if args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "-i" | "--interactive" | "--login" | "-l" | "--watch"
        )
    }) {
        return Err(ToolError::InvalidInput(
            "exec_command".into(),
            "interactive flags are not allowed".into(),
        ));
    }

    if SHELL_PROGRAMS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(program_name))
    {
        let command_arg = args
            .windows(2)
            .find(|pair| matches!(pair[0].as_str(), "-c" | "-lc"))
            .map(|pair| pair[1].as_str());
        let Some(command_arg) = command_arg else {
            return Err(ToolError::InvalidInput(
                "exec_command".into(),
                format!(
                    "shell program '{}' is allowed only with '-c' or '-lc'",
                    program_name
                ),
            ));
        };

        let lowered = command_arg.to_ascii_lowercase();
        if lowered.contains("nohup")
            || lowered.contains("disown")
            || lowered.contains(" tmux")
            || lowered.contains(" screen")
        {
            return Err(ToolError::InvalidInput(
                "exec_command".into(),
                "detached shell execution is not allowed".into(),
            ));
        }
    }

    Ok(())
}

fn basename(program: &str) -> &str {
    Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
}

fn default_timeout_ms() -> u64 {
    kernel_config().tools.timeout_s.max(1) * 1_000
}

fn classify_command_failure(tool_name: &str, detail: &str, timeout_ms: u64) -> ToolError {
    if detail.contains("timed out after") {
        ToolError::Timeout(tool_name.to_string(), timeout_ms)
    } else {
        ToolError::ExecutionFailed(tool_name.to_string(), detail.to_string())
    }
}

fn truncate_stream(text: &str) -> (String, bool) {
    let limit = kernel_config().tools.output_truncate_len;
    if text.len() <= limit {
        return (text.to_string(), false);
    }

    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}... (Output Truncated)", &text[..end]), true)
}

fn render_command_output(
    program: &str,
    args: &[String],
    exit_code: i32,
    duration_ms: u64,
    stdout: &str,
    stderr: &str,
    truncated: bool,
) -> String {
    let mut sections = vec![format!(
        "Command '{}' exited with code {} in {}ms.",
        render_command(program, args),
        exit_code,
        duration_ms
    )];
    if !stdout.is_empty() {
        sections.push(format!("stdout:\n{}", stdout));
    }
    if !stderr.is_empty() {
        sections.push(format!("stderr:\n{}", stderr));
    }
    if truncated {
        sections.push("Output was truncated.".to_string());
    }
    sections.join("\n")
}

fn render_command(program: &str, args: &[String]) -> String {
    if args.is_empty() {
        program.to_string()
    } else {
        format!("{} {}", program, args.join(" "))
    }
}

#[cfg(test)]
#[path = "tests/command.rs"]
mod tests;
