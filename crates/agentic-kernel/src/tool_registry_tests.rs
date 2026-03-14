use std::collections::HashMap;

    use serde_json::json;

    use super::{
        ToolBackendConfig, ToolBackendKind, ToolDescriptor, ToolRegistry, ToolRegistryEntry,
        ToolSource,
    };

    #[test]
    fn builtins_are_registered_and_sorted() {
        let registry = ToolRegistry::with_builtins();
        let names: Vec<String> = registry
            .list()
            .into_iter()
            .map(|item| item.descriptor.name.clone())
            .collect();
        assert_eq!(
            names,
            vec!["calc", "list_files", "python", "read_file", "write_file"]
        );
    }

    #[test]
    fn resolves_legacy_alias_to_canonical_tool() {
        let registry = ToolRegistry::with_builtins();
        let descriptor = registry
            .resolve_invocation_name("PYTHON")
            .expect("python alias");
        assert_eq!(descriptor.descriptor.name, "python");
    }

    #[test]
    fn rejects_backend_kind_mismatch() {
        let mut registry = ToolRegistry::new();
        let result = registry.register(ToolRegistryEntry {
            descriptor: ToolDescriptor {
                name: "runtime_echo".to_string(),
                aliases: vec![],
                description: "echo".to_string(),
                input_schema: json!({"type": "object"}),
                output_schema: json!({"type": "object"}),
                backend_kind: ToolBackendKind::Host,
                capabilities: vec!["echo".to_string()],
                dangerous: false,
                enabled: true,
                source: ToolSource::Runtime,
            },
            backend: ToolBackendConfig::RemoteHttp {
                url: "http://127.0.0.1:8080/tool".to_string(),
                method: "POST".to_string(),
                timeout_ms: 1000,
                headers: HashMap::new(),
            },
        });

        assert!(result.is_err());
    }

    #[test]
    fn cannot_unregister_builtin_tool() {
        let mut registry = ToolRegistry::with_builtins();
        let result = registry.unregister("python");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_runtime_host_backend_registration() {
        let mut registry = ToolRegistry::new();
        let result = registry.register(ToolRegistryEntry {
            descriptor: ToolDescriptor {
                name: "runtime_host_tool".to_string(),
                aliases: vec![],
                description: "host runtime tool".to_string(),
                input_schema: json!({"type": "object"}),
                output_schema: json!({"type": "object"}),
                backend_kind: ToolBackendKind::Host,
                capabilities: vec![],
                dangerous: false,
                enabled: true,
                source: ToolSource::Runtime,
            },
            backend: ToolBackendConfig::Host {
                executor: "external_host_executor".to_string(),
            },
        });

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("not supported for dynamic execution yet"));
    }

    #[test]
    fn rejects_runtime_wasm_backend_registration() {
        let mut registry = ToolRegistry::new();
        let result = registry.register(ToolRegistryEntry {
            descriptor: ToolDescriptor {
                name: "runtime_wasm_tool".to_string(),
                aliases: vec![],
                description: "wasm runtime tool".to_string(),
                input_schema: json!({"type": "object"}),
                output_schema: json!({"type": "object"}),
                backend_kind: ToolBackendKind::Wasm,
                capabilities: vec![],
                dangerous: false,
                enabled: true,
                source: ToolSource::Runtime,
            },
            backend: ToolBackendConfig::Wasm {
                module: "tool.wasm".to_string(),
                export: "run".to_string(),
            },
        });

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("not supported for dynamic execution yet"));
    }
