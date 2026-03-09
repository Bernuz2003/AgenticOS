use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolBackendKind {
    Host,
    Wasm,
    RemoteHttp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolSource {
    BuiltIn,
    Runtime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ToolBackendConfig {
    Host {
        executor: String,
    },
    Wasm {
        module: String,
        export: String,
    },
    RemoteHttp {
        url: String,
        method: String,
        timeout_ms: u64,
        headers: HashMap<String, String>,
    },
}

impl ToolBackendConfig {
    pub fn kind(&self) -> ToolBackendKind {
        match self {
            Self::Host { .. } => ToolBackendKind::Host,
            Self::Wasm { .. } => ToolBackendKind::Wasm,
            Self::RemoteHttp { .. } => ToolBackendKind::RemoteHttp,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub backend_kind: ToolBackendKind,
    pub capabilities: Vec<String>,
    pub dangerous: bool,
    pub enabled: bool,
    pub source: ToolSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRegistryEntry {
    pub descriptor: ToolDescriptor,
    pub backend: ToolBackendConfig,
}

#[derive(Debug, Default)]
pub struct ToolRegistry {
    entries: HashMap<String, ToolRegistryEntry>,
    aliases: HashMap<String, String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        for entry in builtin_entries() {
            registry
                .register(entry)
                .expect("built-in tool descriptors must be valid");
        }
        registry
    }

    pub fn register(&mut self, entry: ToolRegistryEntry) -> Result<(), String> {
        validate_entry(&entry)?;

        let canonical_name = normalize_name(&entry.descriptor.name)?;
        if self.entries.contains_key(&canonical_name) {
            return Err(format!("Tool '{}' is already registered.", entry.descriptor.name));
        }

        let mut normalized_aliases = Vec::new();
        for alias in &entry.descriptor.aliases {
            let normalized_alias = normalize_name(alias)?;
            if normalized_alias == canonical_name {
                continue;
            }
            if self.entries.contains_key(&normalized_alias)
                || self.aliases.contains_key(&normalized_alias)
            {
                return Err(format!("Tool alias '{}' collides with an existing registration.", alias));
            }
            normalized_aliases.push(normalized_alias);
        }

        let mut stored = entry;
        stored.descriptor.name = canonical_name.clone();
        self.entries.insert(canonical_name.clone(), stored);
        for alias in normalized_aliases {
            self.aliases.insert(alias, canonical_name.clone());
        }
        Ok(())
    }

    pub fn unregister(&mut self, name: &str) -> Result<ToolRegistryEntry, String> {
        let canonical = self
            .resolve_canonical_name(name)
            .ok_or_else(|| format!("Tool '{}' is not registered.", name))?;
        let Some(existing) = self.entries.get(&canonical) else {
            return Err(format!("Tool '{}' is not registered.", name));
        };
        if existing.descriptor.source == ToolSource::BuiltIn {
            return Err(format!("Tool '{}' is built-in and cannot be unregistered.", canonical));
        }
        self.aliases.retain(|_, target| target != &canonical);
        self.entries
            .remove(&canonical)
            .ok_or_else(|| format!("Tool '{}' is not registered.", name))
    }

    pub fn get(&self, name: &str) -> Option<&ToolRegistryEntry> {
        let canonical = self.resolve_canonical_name(name)?;
        self.entries.get(&canonical)
    }

    pub fn list(&self) -> Vec<&ToolRegistryEntry> {
        let mut items: Vec<&ToolRegistryEntry> = self.entries.values().collect();
        items.sort_by(|left, right| left.descriptor.name.cmp(&right.descriptor.name));
        items
    }

    pub fn resolve_invocation_name(&self, name: &str) -> Option<&ToolRegistryEntry> {
        self.get(name)
    }

    fn resolve_canonical_name(&self, name: &str) -> Option<String> {
        let normalized = normalize_name(name).ok()?;
        if self.entries.contains_key(&normalized) {
            Some(normalized)
        } else {
            self.aliases.get(&normalized).cloned()
        }
    }
}

fn validate_entry(entry: &ToolRegistryEntry) -> Result<(), String> {
    let descriptor = &entry.descriptor;
    if descriptor.backend_kind != entry.backend.kind() {
        return Err(format!(
            "Tool '{}' declares backend kind '{:?}' but backend config is '{:?}'.",
            descriptor.name,
            descriptor.backend_kind,
            entry.backend.kind()
        ));
    }

    if descriptor.source == ToolSource::Runtime
        && !matches!(entry.backend, ToolBackendConfig::RemoteHttp { .. })
    {
        return Err(format!(
            "Tool '{}' is runtime-registered but backend '{:?}' is not supported for dynamic execution yet.",
            descriptor.name,
            entry.backend.kind()
        ));
    }

    match &entry.backend {
        ToolBackendConfig::Host { executor } => {
            if executor.trim().is_empty() {
                return Err(format!("Tool '{}' host executor cannot be empty.", descriptor.name));
            }
        }
        ToolBackendConfig::Wasm { module, export } => {
            if module.trim().is_empty() || export.trim().is_empty() {
                return Err(format!("Tool '{}' wasm backend must define module and export.", descriptor.name));
            }
        }
        ToolBackendConfig::RemoteHttp {
            url,
            method,
            timeout_ms,
            headers,
        } => {
            if url.trim().is_empty() {
                return Err(format!("Tool '{}' remote_http backend URL cannot be empty.", descriptor.name));
            }
            if method.trim().is_empty() {
                return Err(format!("Tool '{}' remote_http backend method cannot be empty.", descriptor.name));
            }
            if !method.eq_ignore_ascii_case("POST") {
                return Err(format!(
                    "Tool '{}' remote_http backend currently supports only POST.",
                    descriptor.name
                ));
            }
            if *timeout_ms == 0 {
                return Err(format!("Tool '{}' remote_http backend timeout must be > 0.", descriptor.name));
            }
            for (name, value) in headers {
                if name.trim().is_empty() {
                    return Err(format!("Tool '{}' remote_http header names cannot be empty.", descriptor.name));
                }
                if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
                    return Err(format!(
                        "Tool '{}' remote_http headers cannot contain CR/LF characters.",
                        descriptor.name
                    ));
                }
            }
        }
    }

    Ok(())
}

fn normalize_name(name: &str) -> Result<String, String> {
    let normalized = name.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err("Tool name cannot be empty.".to_string());
    }
    if normalized
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-' | '.'))
    {
        Ok(normalized)
    } else {
        Err(format!(
            "Invalid tool name '{}'. Allowed characters: a-z, 0-9, '_', '-', '.'.",
            name
        ))
    }
}

fn builtin_entries() -> Vec<ToolRegistryEntry> {
    vec![
        ToolRegistryEntry {
            descriptor: ToolDescriptor {
                name: "python".to_string(),
                aliases: vec!["PYTHON".to_string()],
                description: "Execute Python code under the syscall sandbox policy.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["code"],
                    "properties": {
                        "code": {"type": "string"}
                    },
                    "additionalProperties": false
                }),
                output_schema: json!({
                    "type": "object",
                    "required": ["output"],
                    "properties": {
                        "output": {"type": "string"}
                    },
                    "additionalProperties": false
                }),
                backend_kind: ToolBackendKind::Host,
                capabilities: vec!["python".to_string(), "sandboxed".to_string()],
                dangerous: true,
                enabled: true,
                source: ToolSource::BuiltIn,
            },
            backend: ToolBackendConfig::Host {
                executor: "builtin_python".to_string(),
            },
        },
        ToolRegistryEntry {
            descriptor: ToolDescriptor {
                name: "write_file".to_string(),
                aliases: vec!["WRITE_FILE".to_string()],
                description: "Write a file inside the workspace root.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["path", "content"],
                    "properties": {
                        "path": {"type": "string"},
                        "content": {"type": "string"}
                    },
                    "additionalProperties": false
                }),
                output_schema: json!({
                    "type": "object",
                    "required": ["output"],
                    "properties": {
                        "output": {"type": "string"}
                    },
                    "additionalProperties": false
                }),
                backend_kind: ToolBackendKind::Host,
                capabilities: vec!["fs".to_string(), "write".to_string()],
                dangerous: true,
                enabled: true,
                source: ToolSource::BuiltIn,
            },
            backend: ToolBackendConfig::Host {
                executor: "builtin_write_file".to_string(),
            },
        },
        ToolRegistryEntry {
            descriptor: ToolDescriptor {
                name: "read_file".to_string(),
                aliases: vec!["READ_FILE".to_string()],
                description: "Read a UTF-8 text file inside the workspace root.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["path"],
                    "properties": {
                        "path": {"type": "string"}
                    },
                    "additionalProperties": false
                }),
                output_schema: json!({
                    "type": "object",
                    "required": ["output"],
                    "properties": {
                        "output": {"type": "string"}
                    },
                    "additionalProperties": false
                }),
                backend_kind: ToolBackendKind::Host,
                capabilities: vec!["fs".to_string(), "read".to_string()],
                dangerous: false,
                enabled: true,
                source: ToolSource::BuiltIn,
            },
            backend: ToolBackendConfig::Host {
                executor: "builtin_read_file".to_string(),
            },
        },
        ToolRegistryEntry {
            descriptor: ToolDescriptor {
                name: "list_files".to_string(),
                aliases: vec!["LS".to_string()],
                description: "List files in the workspace root.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                output_schema: json!({
                    "type": "object",
                    "required": ["output"],
                    "properties": {
                        "output": {"type": "string"}
                    },
                    "additionalProperties": false
                }),
                backend_kind: ToolBackendKind::Host,
                capabilities: vec!["fs".to_string(), "list".to_string()],
                dangerous: false,
                enabled: true,
                source: ToolSource::BuiltIn,
            },
            backend: ToolBackendConfig::Host {
                executor: "builtin_list_files".to_string(),
            },
        },
        ToolRegistryEntry {
            descriptor: ToolDescriptor {
                name: "calc".to_string(),
                aliases: vec!["CALC".to_string()],
                description: "Evaluate a numeric expression through the Python sandbox.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["expression"],
                    "properties": {
                        "expression": {"type": "string"}
                    },
                    "additionalProperties": false
                }),
                output_schema: json!({
                    "type": "object",
                    "required": ["output"],
                    "properties": {
                        "output": {"type": "string"}
                    },
                    "additionalProperties": false
                }),
                backend_kind: ToolBackendKind::Host,
                capabilities: vec!["math".to_string(), "python".to_string()],
                dangerous: false,
                enabled: true,
                source: ToolSource::BuiltIn,
            },
            backend: ToolBackendConfig::Host {
                executor: "builtin_calc".to_string(),
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::{ToolBackendConfig, ToolBackendKind, ToolDescriptor, ToolRegistry, ToolRegistryEntry, ToolSource};

    #[test]
    fn builtins_are_registered_and_sorted() {
        let registry = ToolRegistry::with_builtins();
        let names: Vec<String> = registry
            .list()
            .into_iter()
            .map(|item| item.descriptor.name.clone())
            .collect();
        assert_eq!(names, vec!["calc", "list_files", "python", "read_file", "write_file"]);
    }

    #[test]
    fn resolves_legacy_alias_to_canonical_tool() {
        let registry = ToolRegistry::with_builtins();
        let descriptor = registry.resolve_invocation_name("PYTHON").expect("python alias");
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
        assert!(result.unwrap_err().contains("not supported for dynamic execution yet"));
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
        assert!(result.unwrap_err().contains("not supported for dynamic execution yet"));
    }
}