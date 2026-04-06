use std::env;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Mutex, OnceLock};
use std::thread;

use serde_json::json;

use crate::tool_registry::ToolRegistry;
use crate::tools::executor::{build_structured_invocation, execute_structured_invocation};
use crate::tools::invocation::{
    default_path_grants, ProcessPermissionPolicy, ProcessTrustScope, ToolCaller, ToolContext,
    ToolInvocationTransport,
};
use crate::tools::path_guard::workspace_root;

fn network_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn text_context() -> ToolContext {
    ToolContext {
        pid: Some(1),
        session_id: Some("network-tools".to_string()),
        caller: ToolCaller::AgentText,
        permissions: ProcessPermissionPolicy {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: vec![
                "http_get_json".to_string(),
                "download_url".to_string(),
                "web_fetch".to_string(),
                "web_search".to_string(),
            ],
            path_grants: default_path_grants(),
            path_scopes: vec![".".to_string()],
        },
        transport: ToolInvocationTransport::Structured,
        call_id: None,
    }
}

fn with_network_env<T>(search_base_url: Option<&str>, f: impl FnOnce() -> T) -> T {
    let _guard = network_test_lock().lock().expect("network env lock");
    let previous_hosts = env::var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS").ok();
    let previous_search = env::var("AGENTIC_WEB_SEARCH_BASE_URL").ok();
    env::set_var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS", "127.0.0.1");
    if let Some(search_base_url) = search_base_url {
        env::set_var("AGENTIC_WEB_SEARCH_BASE_URL", search_base_url);
    } else {
        env::remove_var("AGENTIC_WEB_SEARCH_BASE_URL");
    }

    let result = f();

    match previous_hosts {
        Some(value) => env::set_var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS", value),
        None => env::remove_var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS"),
    }
    match previous_search {
        Some(value) => env::set_var("AGENTIC_WEB_SEARCH_BASE_URL", value),
        None => env::remove_var("AGENTIC_WEB_SEARCH_BASE_URL"),
    }

    result
}

fn spawn_single_response_server(
    content_type: &str,
    body: &str,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind http listener");
    let addr = listener.local_addr().expect("listener addr");
    let content_type = content_type.to_string();
    let body = body.to_string();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept client");
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).expect("read request");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            content_type,
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
    });
    (format!("http://{}", addr), handle)
}

#[test]
fn http_get_json_fetches_json_payload() {
    let (base_url, handle) = spawn_single_response_server("application/json", r#"{"ok":true}"#);
    with_network_env(None, || {
        let registry = ToolRegistry::with_builtins();
        let execution = execute_structured_invocation(
            build_structured_invocation(
                "http_get_json",
                json!({ "url": format!("{}/data", base_url) }),
                None,
            )
            .expect("invocation"),
            &text_context(),
            &registry,
        )
        .expect("http_get_json executes");

        assert_eq!(execution.result.output["status_code"], json!(200));
        assert_eq!(execution.result.output["json"], json!({"ok": true}));
    });
    handle.join().expect("join server");
}

#[test]
fn download_url_saves_remote_body_in_workspace() {
    let (base_url, handle) = spawn_single_response_server("text/plain", "downloaded body");
    with_network_env(None, || {
        let registry = ToolRegistry::with_builtins();
        let relative_path = "network_tool_download_test.txt";
        let absolute_path = workspace_root()
            .expect("workspace root")
            .join(relative_path);
        let execution = execute_structured_invocation(
            build_structured_invocation(
                "download_url",
                json!({
                    "url": format!("{}/artifact", base_url),
                    "path": relative_path
                }),
                None,
            )
            .expect("invocation"),
            &text_context(),
            &registry,
        )
        .expect("download_url executes");

        assert_eq!(execution.result.output["bytes_written"], json!(15));
        assert_eq!(
            std::fs::read_to_string(&absolute_path).expect("downloaded file"),
            "downloaded body"
        );

        let _ = std::fs::remove_file(&absolute_path);
    });
    handle.join().expect("join server");
}

#[test]
fn web_fetch_extracts_title_text_and_links() {
    let html = r#"
        <html>
            <head><title>Example Docs</title></head>
            <body>
                <h1>Example Docs</h1>
                <p>Hello <strong>world</strong>.</p>
                <a href="https://example.com/guide">Guide</a>
            </body>
        </html>
    "#;
    let (base_url, handle) = spawn_single_response_server("text/html", html);
    with_network_env(None, || {
        let registry = ToolRegistry::with_builtins();
        let execution = execute_structured_invocation(
            build_structured_invocation(
                "web_fetch",
                json!({ "url": format!("{}/docs", base_url) }),
                None,
            )
            .expect("invocation"),
            &text_context(),
            &registry,
        )
        .expect("web_fetch executes");

        assert_eq!(execution.result.output["title"], json!("Example Docs"));
        assert!(execution.result.output["text"]
            .as_str()
            .unwrap_or("")
            .contains("Hello world."));
        assert_eq!(
            execution.result.output["links"],
            json!(["https://example.com/guide"])
        );
    });
    handle.join().expect("join server");
}

#[test]
fn web_search_parses_provider_results() {
    let body = r#"{
        "Heading": "AgenticOS",
        "AbstractText": "AgenticOS project overview",
        "AbstractURL": "https://example.com/overview",
        "RelatedTopics": [
            {"Text": "AgenticOS Docs - Reference", "FirstURL": "https://example.com/docs"},
            {"Topics": [
                {"Text": "AgenticOS GitHub - Source", "FirstURL": "https://example.com/src"}
            ]}
        ]
    }"#;
    let (base_url, handle) = spawn_single_response_server("application/json", body);
    let search_base = format!("{}/search", base_url);
    with_network_env(Some(&search_base), || {
        let registry = ToolRegistry::with_builtins();
        let execution = execute_structured_invocation(
            build_structured_invocation(
                "web_search",
                json!({ "query": "agentic os", "max_results": 3 }),
                None,
            )
            .expect("invocation"),
            &text_context(),
            &registry,
        )
        .expect("web_search executes");

        assert_eq!(
            execution.result.output["results"],
            json!([
                {
                    "title": "AgenticOS",
                    "url": "https://example.com/overview",
                    "snippet": "AgenticOS project overview",
                    "rank": 1
                },
                {
                    "title": "AgenticOS Docs",
                    "url": "https://example.com/docs",
                    "snippet": "AgenticOS Docs - Reference",
                    "rank": 2
                },
                {
                    "title": "AgenticOS GitHub",
                    "url": "https://example.com/src",
                    "snippet": "AgenticOS GitHub - Source",
                    "rank": 3
                }
            ])
        );
    });
    handle.join().expect("join server");
}
