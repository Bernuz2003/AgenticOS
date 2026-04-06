use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::tool_registry::ToolRegistry;
use crate::tools::executor::{build_structured_invocation, execute_structured_invocation};
use crate::tools::invocation::{
    PathGrantAccessMode, ProcessPathGrant, ProcessPermissionPolicy, ProcessTrustScope, ToolCaller,
    ToolContext, ToolInvocationTransport,
};
use crate::tools::path_guard::workspace_root;

struct DocumentFixture {
    absolute: PathBuf,
    relative: String,
}

impl DocumentFixture {
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

impl Drop for DocumentFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.absolute);
    }
}

fn document_context() -> ToolContext {
    ToolContext {
        pid: Some(7),
        session_id: Some("document-tools".to_string()),
        caller: ToolCaller::AgentText,
        permissions: ProcessPermissionPolicy {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: vec!["inspect_document".to_string()],
            path_grants: vec![ProcessPathGrant {
                root: ".".to_string(),
                access_mode: PathGrantAccessMode::AutonomousWrite,
                capsule: Some("workspace".to_string()),
                label: Some("Workspace".to_string()),
            }],
            path_scopes: vec![".".to_string()],
        },
        transport: ToolInvocationTransport::Structured,
        call_id: None,
    }
}

#[test]
fn inspect_document_parses_json_metadata_and_keys() {
    let fixture = DocumentFixture::new("inspect_document_json");
    let relative_path = fixture.relative_path("payload.json");
    fs::write(
        fixture.absolute.join("payload.json"),
        r#"{"name":"AgenticOS","features":["replay","lineage"],"version":1}"#,
    )
    .expect("write json file");

    let registry = ToolRegistry::with_builtins();
    let execution = execute_structured_invocation(
        build_structured_invocation("inspect_document", json!({ "path": relative_path }), None)
            .expect("invocation"),
        &document_context(),
        &registry,
    )
    .expect("inspect_document executes");

    assert_eq!(execution.result.output["detected_kind"], json!("json"));
    assert_eq!(
        execution.result.output["mime_type"],
        json!("application/json")
    );
    assert_eq!(
        execution.result.output["json_preview"]["object_keys"],
        json!(["features", "name", "version"])
    );
    assert!(
        execution.result.output["sha256"]
            .as_str()
            .unwrap_or("")
            .len()
            >= 64
    );
}

#[test]
fn inspect_document_returns_csv_columns_and_preview_rows() {
    let fixture = DocumentFixture::new("inspect_document_csv");
    let relative_path = fixture.relative_path("report.csv");
    fs::write(
        fixture.absolute.join("report.csv"),
        "name,status\nalpha,ok\nbeta,queued\ngamma,done\n",
    )
    .expect("write csv file");

    let registry = ToolRegistry::with_builtins();
    let execution = execute_structured_invocation(
        build_structured_invocation("inspect_document", json!({ "path": relative_path }), None)
            .expect("invocation"),
        &document_context(),
        &registry,
    )
    .expect("inspect_document executes");

    assert_eq!(execution.result.output["detected_kind"], json!("csv"));
    assert_eq!(
        execution.result.output["csv_preview"]["columns"],
        json!(["name", "status"])
    );
    assert_eq!(
        execution.result.output["csv_preview"]["preview_rows"][0],
        json!(["alpha", "ok"])
    );
}

#[test]
fn inspect_document_detects_pdf_as_metadata_only() {
    let fixture = DocumentFixture::new("inspect_document_pdf");
    let relative_path = fixture.relative_path("sample.pdf");
    fs::write(
        fixture.absolute.join("sample.pdf"),
        b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n",
    )
    .expect("write pdf fixture");

    let registry = ToolRegistry::with_builtins();
    let execution = execute_structured_invocation(
        build_structured_invocation("inspect_document", json!({ "path": relative_path }), None)
            .expect("invocation"),
        &document_context(),
        &registry,
    )
    .expect("inspect_document executes");

    assert_eq!(execution.result.output["detected_kind"], json!("pdf"));
    assert_eq!(
        execution.result.output["mime_type"],
        json!("application/pdf")
    );
    assert!(execution.result.output["preview"].is_null());
    assert!(execution.result.output["json_preview"].is_null());
    assert!(execution.result.output["csv_preview"].is_null());
}
