use std::path::Path;

pub use crate::models::kernel::AgentSessionSummary;

pub fn load_lobby_sessions(workspace_root: &Path) -> Result<Vec<AgentSessionSummary>, String> {
    crate::kernel::history::load_lobby_sessions(workspace_root)
}
