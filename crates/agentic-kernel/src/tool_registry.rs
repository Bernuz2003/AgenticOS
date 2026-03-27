use std::collections::HashMap;

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::tools::invocation::{normalize_tool_name, ToolCaller};
use crate::tools::schema::ensure_valid_schema;

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HostExecutor {
    Python,
    WriteFile,
    Dynamic(String),
}

impl HostExecutor {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Python => "python",
            Self::WriteFile => "write_file",
            Self::Dynamic(name) => name.as_str(),
        }
    }

    pub fn from_binding_name(name: impl Into<String>) -> Result<Self, String> {
        let name = normalize_tool_name(&name.into())?;
        Ok(match name.as_str() {
            "python" => Self::Python,
            "write_file" => Self::WriteFile,
            _ => Self::Dynamic(name),
        })
    }
}

impl Serialize for HostExecutor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for HostExecutor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_binding_name(value).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ToolBackendConfig {
    Host {
        executor: HostExecutor,
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
    #[serde(default)]
    pub input_example: Option<Value>,
    pub output_schema: Value,
    #[serde(default = "default_allowed_callers")]
    pub allowed_callers: Vec<ToolCaller>,
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

#[derive(Debug, Default, Clone)]
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
        for entry in crate::tools::builtins::host_builtin_registry_entries() {
            registry
                .register(entry)
                .expect("built-in tool descriptors must be valid");
        }
        registry
    }

    pub fn register(&mut self, entry: ToolRegistryEntry) -> Result<(), String> {
        validate_entry(&entry)?;

        let canonical_name = normalize_tool_name(&entry.descriptor.name)?;
        if self.entries.contains_key(&canonical_name) {
            return Err(format!(
                "Tool '{}' is already registered.",
                entry.descriptor.name
            ));
        }

        let mut normalized_aliases = Vec::new();
        for alias in &entry.descriptor.aliases {
            let normalized_alias = normalize_tool_name(alias)?;
            if normalized_alias == canonical_name {
                continue;
            }
            if self.entries.contains_key(&normalized_alias)
                || self.aliases.contains_key(&normalized_alias)
            {
                return Err(format!(
                    "Tool alias '{}' collides with an existing registration.",
                    alias
                ));
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
            return Err(format!(
                "Tool '{}' is built-in and cannot be unregistered.",
                canonical
            ));
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
        let normalized = normalize_tool_name(name).ok()?;
        if self.entries.contains_key(&normalized) {
            Some(normalized)
        } else {
            self.aliases.get(&normalized).cloned()
        }
    }
}

fn validate_entry(entry: &ToolRegistryEntry) -> Result<(), String> {
    let descriptor = &entry.descriptor;
    if descriptor.allowed_callers.is_empty() {
        return Err(format!(
            "Tool '{}' must allow at least one caller.",
            descriptor.name
        ));
    }
    ensure_valid_schema(
        &descriptor.input_schema,
        &format!("tool '{}'.input_schema", descriptor.name),
    )?;
    ensure_valid_schema(
        &descriptor.output_schema,
        &format!("tool '{}'.output_schema", descriptor.name),
    )?;
    if let Some(example) = descriptor.input_example.as_ref() {
        crate::tools::schema::validate_value(
            &descriptor.input_schema,
            example,
            &format!("tool '{}'.input_example", descriptor.name),
        )?;
    }

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
            normalize_tool_name(executor.as_str()).map_err(|err| {
                format!(
                    "Tool '{}' host executor binding is invalid: {}",
                    descriptor.name, err
                )
            })?;
        }
        ToolBackendConfig::Wasm { module, export } => {
            if module.trim().is_empty() || export.trim().is_empty() {
                return Err(format!(
                    "Tool '{}' wasm backend must define module and export.",
                    descriptor.name
                ));
            }
        }
        ToolBackendConfig::RemoteHttp {
            url,
            method,
            timeout_ms,
            headers,
        } => {
            if url.trim().is_empty() {
                return Err(format!(
                    "Tool '{}' remote_http backend URL cannot be empty.",
                    descriptor.name
                ));
            }
            if method.trim().is_empty() {
                return Err(format!(
                    "Tool '{}' remote_http backend method cannot be empty.",
                    descriptor.name
                ));
            }
            if !method.eq_ignore_ascii_case("POST") {
                return Err(format!(
                    "Tool '{}' remote_http backend currently supports only POST.",
                    descriptor.name
                ));
            }
            if *timeout_ms == 0 {
                return Err(format!(
                    "Tool '{}' remote_http backend timeout must be > 0.",
                    descriptor.name
                ));
            }
            for (name, value) in headers {
                if name.trim().is_empty() {
                    return Err(format!(
                        "Tool '{}' remote_http header names cannot be empty.",
                        descriptor.name
                    ));
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

fn default_allowed_callers() -> Vec<ToolCaller> {
    vec![ToolCaller::AgentText, ToolCaller::AgentSupervisor]
}

#[cfg(test)]
#[path = "tests/tool_registry.rs"]
mod tests;
