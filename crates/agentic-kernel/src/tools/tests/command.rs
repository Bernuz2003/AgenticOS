use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::tool_registry::ToolRegistry;
use crate::tools::executor::{build_structured_invocation, execute_structured_invocation};
use crate::tools::invocation::{
    default_path_grants, ProcessPermissionPolicy, ProcessTrustScope, ToolCaller, ToolContext,
    ToolInvocationTransport,
};
use crate::tools::path_guard::workspace_root;

struct WorkspaceFixture {
    absolute: PathBuf,
    relative: String,
}

impl WorkspaceFixture {
    fn new(prefix: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_nanos();
        let relative = format!("{prefix}_{unique}");
        let absolute = workspace_root().expect("workspace root").join(&relative);
        fs::create_dir_all(&absolute).expect("create fixture directory");
        Self { absolute, relative }
    }
}

impl Drop for WorkspaceFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.absolute);
    }
}

fn text_context() -> ToolContext {
    ToolContext {
        pid: Some(1),
        session_id: Some("command-tools".to_string()),
        caller: ToolCaller::AgentText,
        permissions: ProcessPermissionPolicy {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: vec!["exec_command".to_string()],
            path_grants: default_path_grants(),
            path_scopes: vec![".".to_string()],
        },
        transport: ToolInvocationTransport::Structured,
        call_id: None,
    }
}

#[test]
fn exec_command_runs_non_interactive_shell_command() {
    let fixture = WorkspaceFixture::new("exec_command_tool");
    let registry = ToolRegistry::with_builtins();

    let execution = execute_structured_invocation(
        build_structured_invocation(
            "exec_command",
            json!({
                "program": "sh",
                "args": ["-lc", "pwd"],
                "cwd": fixture.relative,
                "timeout_ms": 5000
            }),
            None,
        )
        .expect("invocation"),
        &text_context(),
        &registry,
    )
    .expect("exec_command executes");

    assert_eq!(execution.result.output["exit_code"], json!(0));
    assert_eq!(execution.result.output["successful"], json!(true));
    assert!(execution.result.output["stdout"]
        .as_str()
        .unwrap_or("")
        .trim_end()
        .ends_with(&fixture.relative));
}

#[test]
fn exec_command_rejects_interactive_programs() {
    let registry = ToolRegistry::with_builtins();
    let err = execute_structured_invocation(
        build_structured_invocation(
            "exec_command",
            json!({
                "program": "nano",
                "args": ["notes.txt"]
            }),
            None,
        )
        .expect("invocation"),
        &text_context(),
        &registry,
    )
    .expect_err("interactive command denied");

    assert!(matches!(
        err,
        crate::tools::error::ToolError::InvalidInput(_, _)
    ));
}
