use crate::mcp::models::McpRoot;
use crate::tools::invocation::ToolContext;
use crate::tools::path_guard::resolve_context_grant_roots;

pub(crate) fn roots_for_context(context: &ToolContext) -> Result<Vec<McpRoot>, String> {
    let roots = resolve_context_grant_roots(context)?;
    let mut values = Vec::new();

    for (index, path) in roots.iter().enumerate() {
        let grant = context.permissions.path_grants.get(index);
        values.push(McpRoot {
            uri: file_uri_for_path(path.as_path()),
            name: grant.and_then(|item| item.label.clone().or_else(|| item.capsule.clone())),
        });
    }

    Ok(values)
}

fn file_uri_for_path(path: &std::path::Path) -> String {
    let display = path.to_string_lossy().replace('\\', "/");
    let encoded = display
        .bytes()
        .flat_map(percent_encode_byte)
        .collect::<String>();
    format!("file://{encoded}")
}

fn percent_encode_byte(byte: u8) -> Vec<char> {
    if matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/') {
        return vec![byte as char];
    }

    format!("%{byte:02X}").chars().collect()
}

#[cfg(test)]
mod tests {
    use super::roots_for_context;
    use crate::tools::invocation::{
        default_path_grants, ProcessPermissionPolicy, ProcessTrustScope, ToolCaller, ToolContext,
        ToolInvocationTransport,
    };

    #[test]
    fn converts_path_grants_to_file_roots() {
        let roots = roots_for_context(&ToolContext {
            pid: Some(1),
            session_id: None,
            caller: ToolCaller::AgentText,
            permissions: ProcessPermissionPolicy {
                trust_scope: ProcessTrustScope::InteractiveChat,
                actions_allowed: false,
                allowed_tools: vec!["read_file".to_string()],
                path_grants: default_path_grants(),
                path_scopes: vec![".".to_string()],
            },
            transport: ToolInvocationTransport::Structured,
            call_id: None,
        })
        .expect("roots");

        assert_eq!(roots.len(), 1);
        assert!(roots[0].uri.starts_with("file:///"));
    }
}
