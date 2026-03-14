use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use super::{handle_syscall, parse_tool_invocation, SyscallRateMap};
    use crate::tool_registry::{
        ToolBackendConfig, ToolBackendKind, ToolDescriptor, ToolRegistry, ToolRegistryEntry,
        ToolSource,
    };
    use serde_json::json;
    use std::sync::{Mutex, OnceLock};

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn spawn_mock_remote_tool_server(body: &str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock remote tool server");
        let address = listener.local_addr().expect("remote tool local addr");
        let response_body = body.to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept remote tool request");
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).expect("read remote tool request");
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write remote tool response");
            request
        });
        (format!("http://{}/invoke", address), handle)
    }

    fn register_remote_tool(registry: &mut ToolRegistry, url: String) {
        registry
            .register(ToolRegistryEntry {
                descriptor: ToolDescriptor {
                    name: "remote_echo".to_string(),
                    aliases: vec!["REMOTE_ECHO".to_string()],
                    description: "Forward payload to a remote HTTP tool.".to_string(),
                    input_schema: json!({"type": "object"}),
                    output_schema: json!({
                        "type": "object",
                        "required": ["output"],
                        "properties": {
                            "output": {"type": "string"}
                        },
                        "additionalProperties": false
                    }),
                    backend_kind: ToolBackendKind::RemoteHttp,
                    capabilities: vec!["remote".to_string()],
                    dangerous: false,
                    enabled: true,
                    source: ToolSource::Runtime,
                },
                backend: ToolBackendConfig::RemoteHttp {
                    url,
                    method: "POST".to_string(),
                    timeout_ms: 1_000,
                    headers: HashMap::from([(
                        "X-AgenticOS-Tool".to_string(),
                        "remote_echo".to_string(),
                    )]),
                },
            })
            .expect("register remote tool");
    }

    #[test]
    fn denies_path_traversal() {
        let _guard = test_lock().lock().unwrap();
        let mut rate_map = SyscallRateMap::new();
        let registry = ToolRegistry::with_builtins();

        let out = handle_syscall("READ_FILE: ../secret.txt", 10, &mut rate_map, &registry);
        assert!(out.output.contains("Path traversal") || out.output.contains("escapes workspace"));
    }

    #[test]
    fn rate_limit_can_kill_process() {
        let _guard = test_lock().lock().unwrap();
        let mut rate_map = SyscallRateMap::new();
        let registry = ToolRegistry::with_builtins();
        std::env::set_var("AGENTIC_SYSCALL_MAX_PER_WINDOW", "1");
        std::env::set_var("AGENTIC_SYSCALL_WINDOW_S", "60");

        let _ = handle_syscall("LS", 22, &mut rate_map, &registry);
        let second = handle_syscall("LS", 22, &mut rate_map, &registry);
        assert!(second.should_kill_process);
        assert!(second.output.contains("Rate limit exceeded"));

        std::env::remove_var("AGENTIC_SYSCALL_MAX_PER_WINDOW");
        std::env::remove_var("AGENTIC_SYSCALL_WINDOW_S");
    }

    #[test]
    fn disable_host_fallback_rejects_unavailable_wasm_runner() {
        let _guard = test_lock().lock().unwrap();
        let mut rate_map = SyscallRateMap::new();
        let registry = ToolRegistry::with_builtins();
        std::env::set_var("AGENTIC_SANDBOX_MODE", "wasm");
        std::env::set_var("AGENTIC_ALLOW_HOST_FALLBACK", "false");

        let out = handle_syscall("PYTHON: print('x')", 31, &mut rate_map, &registry);
        assert!(out.output.contains("wasm") || out.output.contains("fallback disabled"));

        std::env::remove_var("AGENTIC_SANDBOX_MODE");
        std::env::remove_var("AGENTIC_ALLOW_HOST_FALLBACK");
    }

    #[test]
    fn parses_canonical_tool_invocation() {
        let parsed = parse_tool_invocation(r#"TOOL:python {"code":"print(1)"}"#)
            .expect("canonical tool invocation");
        assert_eq!(parsed.name, "python");
        assert_eq!(parsed.input["code"], "print(1)");
    }

    #[test]
    fn invokes_registered_remote_http_tool() {
        let _guard = test_lock().lock().unwrap();
        let mut rate_map = SyscallRateMap::new();
        let mut registry = ToolRegistry::with_builtins();
        let (url, handle) = spawn_mock_remote_tool_server(r#"{"output":"remote ok"}"#);
        register_remote_tool(&mut registry, url);

        std::env::set_var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS", "127.0.0.1");
        let out = handle_syscall(
            r#"TOOL:remote_echo {"message":"hello"}"#,
            44,
            &mut rate_map,
            &registry,
        );
        std::env::remove_var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS");

        assert_eq!(out.output, "remote ok");
        assert!(!out.should_kill_process);

        let request = handle.join().expect("join mock remote tool server");
        assert!(request.contains("POST /invoke HTTP/1.1"));
        assert!(request.contains("X-AgenticOS-Tool: remote_echo"));
        assert!(request.contains(r#"{"message":"hello"}"#));
    }

    #[test]
    fn rejects_remote_http_tool_when_host_is_not_allowlisted() {
        let _guard = test_lock().lock().unwrap();
        let mut rate_map = SyscallRateMap::new();
        let mut registry = ToolRegistry::with_builtins();
        register_remote_tool(&mut registry, "http://127.0.0.1:18081/invoke".to_string());

        std::env::set_var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS", "example.com");
        let out = handle_syscall(
            r#"TOOL:remote_echo {"message":"hello"}"#,
            45,
            &mut rate_map,
            &registry,
        );
        std::env::remove_var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS");

        assert!(out.output.contains("not allowlisted"));
        assert!(!out.should_kill_process);
    }

    #[test]
    fn rejects_remote_http_hostname_resolving_to_loopback() {
        let _guard = test_lock().lock().unwrap();
        let mut rate_map = SyscallRateMap::new();
        let mut registry = ToolRegistry::with_builtins();
        register_remote_tool(&mut registry, "http://localhost:18081/invoke".to_string());

        std::env::set_var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS", "localhost");
        let out = handle_syscall(
            r#"TOOL:remote_echo {"message":"hello"}"#,
            47,
            &mut rate_map,
            &registry,
        );
        std::env::remove_var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS");

        assert!(out.output.contains("disallowed address"));
        assert!(!out.should_kill_process);
    }

    #[test]
    fn rejects_remote_http_tool_response_over_limit() {
        let _guard = test_lock().lock().unwrap();
        let mut rate_map = SyscallRateMap::new();
        let mut registry = ToolRegistry::with_builtins();
        let oversized_body = format!(r#"{{"output":"{}"}}"#, "x".repeat(1024));
        let (url, handle) = spawn_mock_remote_tool_server(&oversized_body);
        register_remote_tool(&mut registry, url);

        std::env::set_var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS", "127.0.0.1");
        std::env::set_var("AGENTIC_REMOTE_TOOL_MAX_RESPONSE_BYTES", "256");
        let out = handle_syscall(
            r#"TOOL:remote_echo {"message":"hello"}"#,
            46,
            &mut rate_map,
            &registry,
        );
        std::env::remove_var("AGENTIC_REMOTE_TOOL_ALLOWED_HOSTS");
        std::env::remove_var("AGENTIC_REMOTE_TOOL_MAX_RESPONSE_BYTES");

        assert!(out.output.contains("exceeded limit"));
        let _ = handle.join();
    }
