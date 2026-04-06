use std::collections::HashSet;
use std::path::Path;

use rusqlite::{params, Connection};

use crate::models::kernel::AgentSessionSummary;
use crate::utils::time::relative_age_label;

use super::db::{load_all_session_identities, open_connection, table_exists, SessionIdentity};

pub fn load_lobby_sessions(workspace_root: &Path) -> Result<Vec<AgentSessionSummary>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(Vec::new());
    };
    let replay_branch_session_ids = load_replay_branch_session_ids(&connection)?;
    load_all_session_identities(&connection)?
        .into_iter()
        .filter(|identity| !replay_branch_session_ids.contains(&identity.session_id))
        .map(agent_session_summary_from_identity)
        .collect()
}

pub fn delete_session(workspace_root: &Path, session_id: &str) -> Result<(), String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(());
    };

    connection
        .execute("PRAGMA foreign_keys = ON", [])
        .map_err(|err| err.to_string())?;

    connection
        .execute(
            "DELETE FROM sessions WHERE session_id = ?1",
            params![session_id],
        )
        .map_err(|err| err.to_string())?;

    Ok(())
}

pub(crate) fn agent_session_summary_from_identity(
    identity: SessionIdentity,
) -> Result<AgentSessionSummary, String> {
    let pid = identity.active_pid.or(identity.last_pid).unwrap_or(0);
    Ok(AgentSessionSummary {
        session_id: identity.session_id,
        pid,
        active_pid: identity.active_pid,
        last_pid: identity.last_pid,
        title: identity.title,
        prompt_preview: identity.prompt_preview,
        status: if identity.active_pid.is_some() || identity.status == "running" {
            "running".to_string()
        } else {
            "idle".to_string()
        },
        runtime_state: None,
        uptime_label: relative_age_label(identity.updated_at_ms),
        tokens_label: if identity.turn_count == 0 {
            "0".to_string()
        } else {
            format!("{}t", identity.turn_count)
        },
        context_strategy: "sliding_window".to_string(),
        runtime_id: identity.runtime_id,
        runtime_label: identity.runtime_label,
        backend_class: identity.backend_class,
        orchestration_id: None,
        orchestration_task_id: None,
    })
}

fn load_replay_branch_session_ids(connection: &Connection) -> Result<HashSet<String>, String> {
    if !table_exists(connection, "replay_branch_index")? {
        return Ok(HashSet::new());
    }

    let mut statement = connection
        .prepare("SELECT session_id FROM replay_branch_index")
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|err| err.to_string())?;

    let mut session_ids = HashSet::new();
    for row in rows {
        session_ids.insert(row.map_err(|err| err.to_string())?);
    }
    Ok(session_ids)
}
