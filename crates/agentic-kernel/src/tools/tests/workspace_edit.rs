use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::tool_registry::ToolRegistry;
use crate::tools::executor::{build_structured_invocation, execute_structured_invocation};
use crate::tools::invocation::{
    ProcessPermissionPolicy, ProcessTrustScope, ToolCaller, ToolContext, ToolInvocationTransport,
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

    fn relative_path(&self, child: &str) -> String {
        format!("{}/{}", self.relative, child)
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
        session_id: Some("workspace-edit-tools".to_string()),
        caller: ToolCaller::AgentText,
        permissions: ProcessPermissionPolicy {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: vec![
                "append_file".to_string(),
                "replace_in_file".to_string(),
                "list_tree".to_string(),
                "diff_files".to_string(),
            ],
            path_scopes: vec![".".to_string()],
        },
        transport: ToolInvocationTransport::Structured,
        call_id: None,
    }
}

#[test]
fn append_file_appends_and_reports_creation() {
    let fixture = WorkspaceFixture::new("append_file_tool");
    let registry = ToolRegistry::with_builtins();

    let first = execute_structured_invocation(
        build_structured_invocation(
            "append_file",
            json!({
                "path": fixture.relative_path("log.txt"),
                "content": "first\n"
            }),
            None,
        )
        .expect("invocation"),
        &text_context(),
        &registry,
    )
    .expect("append first");
    let second = execute_structured_invocation(
        build_structured_invocation(
            "append_file",
            json!({
                "path": fixture.relative_path("log.txt"),
                "content": "second\n"
            }),
            None,
        )
        .expect("invocation"),
        &text_context(),
        &registry,
    )
    .expect("append second");

    assert_eq!(first.result.output["created"], json!(true));
    assert_eq!(second.result.output["created"], json!(false));
    assert_eq!(
        fs::read_to_string(fixture.absolute.join("log.txt")).expect("appended file"),
        "first\nsecond\n"
    );
}

#[test]
fn replace_in_file_replaces_single_exact_match() {
    let fixture = WorkspaceFixture::new("replace_file_tool");
    fs::write(fixture.absolute.join("notes.txt"), "alpha\nbeta\ngamma\n").expect("write notes");
    let registry = ToolRegistry::with_builtins();

    let result = execute_structured_invocation(
        build_structured_invocation(
            "replace_in_file",
            json!({
                "path": fixture.relative_path("notes.txt"),
                "find": "beta",
                "replace": "delta",
                "replace_all": false
            }),
            None,
        )
        .expect("invocation"),
        &text_context(),
        &registry,
    )
    .expect("replace_in_file");

    assert_eq!(result.result.output["replacements"], json!(1));
    assert_eq!(
        fs::read_to_string(fixture.absolute.join("notes.txt")).expect("replaced file"),
        "alpha\ndelta\ngamma\n"
    );
}

#[test]
fn list_tree_returns_depth_limited_structure() {
    let fixture = WorkspaceFixture::new("list_tree_tool");
    fs::create_dir_all(fixture.absolute.join("nested/deeper")).expect("create dirs");
    fs::write(fixture.absolute.join("nested/file.txt"), "hello").expect("write file");
    fs::write(fixture.absolute.join("nested/deeper/skip.txt"), "skip").expect("write deep file");
    let registry = ToolRegistry::with_builtins();

    let result = execute_structured_invocation(
        build_structured_invocation(
            "list_tree",
            json!({
                "path": fixture.relative,
                "max_depth": 1,
                "max_entries": 20
            }),
            None,
        )
        .expect("invocation"),
        &text_context(),
        &registry,
    )
    .expect("list_tree");

    let entries = result.result.output["entries"]
        .as_array()
        .expect("entries array");
    assert!(entries
        .iter()
        .any(|entry| entry["entry_type"] == json!("directory")));
    assert!(entries
        .iter()
        .any(|entry| entry["path"] == json!(fixture.relative_path("nested"))));
    assert!(!entries
        .iter()
        .any(|entry| entry["path"] == json!(fixture.relative_path("nested/deeper/skip.txt"))));
}

#[test]
fn diff_files_reports_changed_lines() {
    let fixture = WorkspaceFixture::new("diff_files_tool");
    fs::write(fixture.absolute.join("left.txt"), "one\ntwo\nthree\n").expect("write left");
    fs::write(
        fixture.absolute.join("right.txt"),
        "one\nTWO\nthree\nfour\n",
    )
    .expect("write right");
    let registry = ToolRegistry::with_builtins();

    let result = execute_structured_invocation(
        build_structured_invocation(
            "diff_files",
            json!({
                "left_path": fixture.relative_path("left.txt"),
                "right_path": fixture.relative_path("right.txt"),
                "max_changes": 10
            }),
            None,
        )
        .expect("invocation"),
        &text_context(),
        &registry,
    )
    .expect("diff_files");

    assert_eq!(result.result.output["identical"], json!(false));
    assert_eq!(
        result.result.output["changes"],
        json!([
            {"line": 2, "left": "two", "right": "TWO"},
            {"line": 4, "left": null, "right": "four"}
        ])
    );
}
