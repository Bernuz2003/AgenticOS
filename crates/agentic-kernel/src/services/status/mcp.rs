use agentic_control_models::{
    McpDiscoveredToolView, McpPromptView, McpResourceView, McpServerStatusView, McpStatusView,
};

use super::view::StatusSnapshotDeps;

pub(super) fn build_mcp_status_view(deps: &StatusSnapshotDeps<'_>) -> Option<McpStatusView> {
    let snapshot = deps.mcp_bridge.and_then(|bridge| bridge.status_snapshot())?;

    Some(McpStatusView {
        servers: snapshot
            .servers
            .into_iter()
            .map(|server| McpServerStatusView {
                server_id: server.server_id,
                label: server.label,
                transport: server.transport,
                trust_level: server.trust_level,
                auth_mode: server.auth_mode,
                health: server.health,
                tool_prefix: server.tool_prefix,
                enabled: server.enabled,
                connected: server.connected,
                default_allowlisted: server.default_allowlisted,
                approval_required: server.approval_required,
                roots_enabled: server.roots_enabled,
                exposed_tools: server.exposed_tools,
                discovered_tools: server
                    .discovered_tools
                    .into_iter()
                    .map(|tool| McpDiscoveredToolView {
                        agentic_tool_name: tool.agentic_tool_name,
                        target_name: tool.target_name,
                        title: tool.title,
                        description: tool.description,
                        dangerous: tool.dangerous,
                        default_allowlisted: tool.default_allowlisted,
                        approval_required: tool.approval_required,
                        read_only_hint: tool.read_only_hint,
                        destructive_hint: tool.destructive_hint,
                        idempotent_hint: tool.idempotent_hint,
                        open_world_hint: tool.open_world_hint,
                    })
                    .collect(),
                prompts: server
                    .prompts
                    .into_iter()
                    .map(|prompt| McpPromptView {
                        name: prompt.name,
                        title: prompt.title,
                        description: prompt.description,
                    })
                    .collect(),
                resources: server
                    .resources
                    .into_iter()
                    .map(|resource| McpResourceView {
                        name: resource.name,
                        title: resource.title,
                        uri: resource.uri,
                        description: resource.description,
                        mime_type: resource.mime_type,
                    })
                    .collect(),
                last_latency_ms: server.last_latency_ms,
                last_error: server.last_error,
            })
            .collect(),
    })
}
