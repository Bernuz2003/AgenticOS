use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::tool_registry::ToolRegistry;
use crate::tools::executor::{
    build_structured_invocation, execute_structured_invocation, execute_text_invocation,
};
use crate::tools::invocation::{
    default_path_grants, PathGrantAccessMode, ProcessPathGrant, ProcessPermissionPolicy,
    ProcessTrustScope, ToolCaller, ToolContext, ToolInvocationTransport,
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

struct ExternalFixture {
    absolute: PathBuf,
}

impl ExternalFixture {
    fn new(prefix: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_nanos();
        let absolute = std::env::temp_dir().join(format!("{prefix}_{unique}"));
        fs::create_dir_all(&absolute).expect("create external fixture directory");
        Self { absolute }
    }

    fn grant_root(&self) -> String {
        self.absolute.to_string_lossy().replace('\\', "/")
    }
}

impl Drop for ExternalFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.absolute);
    }
}

fn text_context() -> ToolContext {
    ToolContext {
        pid: Some(1),
        session_id: Some("workspace-tools".to_string()),
        caller: ToolCaller::AgentText,
        permissions: ProcessPermissionPolicy {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: vec![
                "path_info".to_string(),
                "find_files".to_string(),
                "search_text".to_string(),
                "read_file_range".to_string(),
                "mkdir".to_string(),
                "read_file".to_string(),
                "write_file".to_string(),
                "list_files".to_string(),
                "calc".to_string(),
            ],
            path_grants: default_path_grants(),
            path_scopes: vec![".".to_string()],
        },
        transport: ToolInvocationTransport::Text,
        call_id: None,
    }
}

fn scoped_text_context(scope: String) -> ToolContext {
    ToolContext {
        permissions: ProcessPermissionPolicy {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: vec!["read_file".to_string()],
            path_grants: vec![ProcessPathGrant {
                root: scope.clone(),
                access_mode: PathGrantAccessMode::AutonomousWrite,
                capsule: Some("workspace".to_string()),
                label: Some("Scoped test root".to_string()),
            }],
            path_scopes: vec![scope],
        },
        ..text_context()
    }
}

fn granted_text_context(
    root: String,
    access_mode: PathGrantAccessMode,
    allowed_tools: &[&str],
) -> ToolContext {
    ToolContext {
        permissions: ProcessPermissionPolicy {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: allowed_tools
                .iter()
                .map(|tool| (*tool).to_string())
                .collect(),
            path_grants: vec![ProcessPathGrant {
                root: root.clone(),
                access_mode,
                capsule: Some("host_fs".to_string()),
                label: Some("External test root".to_string()),
            }],
            path_scopes: vec![root],
        },
        ..text_context()
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

#[test]
fn read_file_rejects_paths_outside_process_scope() {
    let inside = WorkspaceFixture::new("read_scope_inside");
    let outside = WorkspaceFixture::new("read_scope_outside");
    fs::write(inside.absolute.join("allowed.txt"), "inside").expect("write inside");
    fs::write(outside.absolute.join("blocked.txt"), "outside").expect("write outside");

    let registry = ToolRegistry::with_builtins();
    let err = execute_structured_invocation(
        build_structured_invocation(
            "read_file",
            json!({ "path": outside.relative_path("blocked.txt") }),
            None,
        )
        .expect("structured invocation"),
        &ToolContext {
            transport: ToolInvocationTransport::Structured,
            ..scoped_text_context(inside.relative.clone())
        },
        &registry,
    )
    .expect_err("scope enforcement");

    assert!(matches!(
        err,
        crate::tools::error::ToolError::ExecutionFailed(_, _)
    ));
}

#[test]
fn read_file_accepts_absolute_external_grant() {
    let fixture = ExternalFixture::new("read_external_grant");
    let absolute_file = fixture.absolute.join("allowed.txt");
    fs::write(&absolute_file, "external").expect("write external file");

    let registry = ToolRegistry::with_builtins();
    let result = execute_structured_invocation(
        build_structured_invocation(
            "read_file",
            json!({ "path": absolute_file.to_string_lossy() }),
            None,
        )
        .expect("structured invocation"),
        &ToolContext {
            transport: ToolInvocationTransport::Structured,
            ..granted_text_context(
                fixture.grant_root(),
                PathGrantAccessMode::ReadOnly,
                &["read_file"],
            )
        },
        &registry,
    )
    .expect("external grant read succeeds");

    assert_eq!(result.result.output["output"], json!("external"));
}

#[test]
fn write_file_accepts_absolute_external_autonomous_grant() {
    let fixture = ExternalFixture::new("write_external_grant");
    let absolute_file = fixture.absolute.join("written.txt");

    let registry = ToolRegistry::with_builtins();
    let result = execute_structured_invocation(
        build_structured_invocation(
            "write_file",
            json!({
                "path": absolute_file.to_string_lossy(),
                "content": "external write"
            }),
            None,
        )
        .expect("structured invocation"),
        &ToolContext {
            transport: ToolInvocationTransport::Structured,
            ..granted_text_context(
                fixture.grant_root(),
                PathGrantAccessMode::AutonomousWrite,
                &["write_file"],
            )
        },
        &registry,
    )
    .expect("external write succeeds");

    assert_eq!(result.result.output["bytes_written"], json!(14));
    assert_eq!(
        fs::read_to_string(&absolute_file).expect("written external file"),
        "external write"
    );
}

#[test]
fn write_file_rejects_read_only_absolute_external_grant() {
    let fixture = ExternalFixture::new("write_external_read_only");
    let absolute_file = fixture.absolute.join("blocked.txt");

    let registry = ToolRegistry::with_builtins();
    let err = execute_structured_invocation(
        build_structured_invocation(
            "write_file",
            json!({
                "path": absolute_file.to_string_lossy(),
                "content": "blocked"
            }),
            None,
        )
        .expect("structured invocation"),
        &ToolContext {
            transport: ToolInvocationTransport::Structured,
            ..granted_text_context(
                fixture.grant_root(),
                PathGrantAccessMode::ReadOnly,
                &["write_file"],
            )
        },
        &registry,
    )
    .expect_err("read-only grant denies write");

    match err {
        crate::tools::error::ToolError::ExecutionFailed(_, detail) => {
            assert!(detail.contains("read-only under grants"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(!absolute_file.exists());
}
