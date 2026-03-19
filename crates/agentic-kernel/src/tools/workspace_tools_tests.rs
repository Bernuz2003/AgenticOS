use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;

use super::workspace_root;
use crate::tool_registry::ToolRegistry;
use crate::tools::executor::{
    build_structured_invocation, execute_structured_invocation, execute_text_invocation,
};
use crate::tools::invocation::{ToolCaller, ToolContext, ToolInvocationTransport};

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
        session_id: Some("workspace-tools".to_string()),
        caller: ToolCaller::AgentText,
        transport: ToolInvocationTransport::Text,
        call_id: None,
    }
}

#[test]
fn path_info_reports_existing_and_missing_paths() {
    let fixture = WorkspaceFixture::new("path_info_tool");
    let file_relative = fixture.relative_path("notes.txt");
    fs::write(fixture.absolute.join("notes.txt"), "hello").expect("write file");

    let registry = ToolRegistry::with_builtins();
    let existing = execute_structured_invocation(
        build_structured_invocation("path_info", json!({ "path": file_relative }), None)
            .expect("structured invocation"),
        &ToolContext {
            transport: ToolInvocationTransport::Structured,
            ..text_context()
        },
        &registry,
    )
    .expect("path_info existing");

    assert_eq!(existing.result.output["exists"], json!(true));
    assert_eq!(existing.result.output["entry_type"], json!("file"));
    assert_eq!(existing.result.output["size_bytes"], json!(5));

    let missing = execute_structured_invocation(
        build_structured_invocation(
            "path_info",
            json!({ "path": fixture.relative_path("missing.txt") }),
            None,
        )
        .expect("structured invocation"),
        &ToolContext {
            transport: ToolInvocationTransport::Structured,
            ..text_context()
        },
        &registry,
    )
    .expect("path_info missing");

    assert_eq!(missing.result.output["exists"], json!(false));
}

#[test]
fn find_files_filters_by_pattern_and_extension() {
    let fixture = WorkspaceFixture::new("find_files_tool");
    fs::write(fixture.absolute.join("alpha.rs"), "fn alpha() {}\n").expect("write alpha");
    fs::write(fixture.absolute.join("beta.txt"), "beta\n").expect("write beta");
    fs::create_dir_all(fixture.absolute.join("nested")).expect("create nested");
    fs::write(fixture.absolute.join("nested/gamma.rs"), "fn gamma() {}\n").expect("write gamma");

    let registry = ToolRegistry::with_builtins();
    let result = execute_structured_invocation(
        build_structured_invocation(
            "find_files",
            json!({
                "path": fixture.relative.clone(),
                "pattern": "ga",
                "extension": "rs",
                "recursive": true,
                "max_results": 10
            }),
            None,
        )
        .expect("structured invocation"),
        &ToolContext {
            transport: ToolInvocationTransport::Structured,
            ..text_context()
        },
        &registry,
    )
    .expect("find_files executes");

    assert_eq!(
        result.result.output["matches"],
        json!([fixture.relative_path("nested/gamma.rs")])
    );
}

#[test]
fn search_text_matches_on_text_and_structured_paths() {
    let fixture = WorkspaceFixture::new("search_text_tool");
    fs::write(
        fixture.absolute.join("main.rs"),
        "fn main() {\n    let needle = 1;\n}\n",
    )
    .expect("write main");
    fs::create_dir_all(fixture.absolute.join("nested")).expect("create nested");
    fs::write(
        fixture.absolute.join("nested/lib.rs"),
        "pub fn helper() {\n    let needle = 2;\n}\n",
    )
    .expect("write lib");

    let registry = ToolRegistry::with_builtins();
    let text = execute_text_invocation(
        &format!(
            r#"TOOL:search_text {{"query":"needle","path":"{}","recursive":true,"max_results":10}}"#,
            fixture.relative
        ),
        &text_context(),
        &registry,
    )
    .expect("text invocation");
    let structured = execute_structured_invocation(
        build_structured_invocation(
            "search_text",
            json!({
                "query": "needle",
                "path": fixture.relative.clone(),
                "recursive": true,
                "max_results": 10
            }),
            None,
        )
        .expect("structured invocation"),
        &ToolContext {
            transport: ToolInvocationTransport::Structured,
            ..text_context()
        },
        &registry,
    )
    .expect("structured invocation");

    assert_eq!(text.result.output, structured.result.output);
    assert_eq!(text.result.display_text, structured.result.display_text);
    assert_eq!(
        structured.result.output["matches"]
            .as_array()
            .map(|v| v.len()),
        Some(2)
    );
}

#[test]
fn read_file_range_returns_requested_lines_with_numbers() {
    let fixture = WorkspaceFixture::new("read_file_range_tool");
    fs::write(
        fixture.absolute.join("notes.txt"),
        "one\ntwo\nthree\nfour\nfive\n",
    )
    .expect("write notes");

    let registry = ToolRegistry::with_builtins();
    let result = execute_structured_invocation(
        build_structured_invocation(
            "read_file_range",
            json!({
                "path": fixture.relative_path("notes.txt"),
                "start_line": 2,
                "end_line": 4
            }),
            None,
        )
        .expect("structured invocation"),
        &ToolContext {
            transport: ToolInvocationTransport::Structured,
            ..text_context()
        },
        &registry,
    )
    .expect("read_file_range executes");

    assert_eq!(result.result.output["start_line"], json!(2));
    assert_eq!(result.result.output["end_line"], json!(4));
    assert_eq!(
        result.result.output["lines"],
        json!([
            {"number": 2, "text": "two"},
            {"number": 3, "text": "three"},
            {"number": 4, "text": "four"}
        ])
    );
    assert!(result
        .result
        .display_text
        .as_deref()
        .unwrap_or("")
        .contains("2: two"));
}

#[test]
fn mkdir_creates_directory_and_is_idempotent() {
    let fixture = WorkspaceFixture::new("mkdir_tool");
    let target_relative = fixture.relative_path("nested/new_dir");
    let registry = ToolRegistry::with_builtins();

    let first = execute_structured_invocation(
        build_structured_invocation(
            "mkdir",
            json!({
                "path": target_relative,
                "create_parents": true
            }),
            None,
        )
        .expect("structured invocation"),
        &ToolContext {
            transport: ToolInvocationTransport::Structured,
            ..text_context()
        },
        &registry,
    )
    .expect("mkdir first");

    let created_path = fixture.absolute.join("nested/new_dir");
    assert!(created_path.is_dir());
    assert_eq!(first.result.output["created"], json!(true));

    let second = execute_structured_invocation(
        build_structured_invocation(
            "mkdir",
            json!({
                "path": fixture.relative_path("nested/new_dir"),
                "create_parents": true
            }),
            None,
        )
        .expect("structured invocation"),
        &ToolContext {
            transport: ToolInvocationTransport::Structured,
            ..text_context()
        },
        &registry,
    )
    .expect("mkdir second");

    assert_eq!(second.result.output["created"], json!(false));
}
