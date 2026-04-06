use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::models::kernel::{
    WorkspaceLineageBranch, WorkspaceLineageBranchKind, WorkspaceLineageSnapshot, WorkspaceSnapshot,
};

use super::db::{load_session_identity, open_connection, table_exists};

#[derive(Debug)]
struct ReplayBranchLink {
    source_session_id: Option<String>,
    source_dump_id: String,
}

#[derive(Debug)]
struct ReplayBranchDescriptor {
    session_id: String,
    source_dump_id: String,
}

pub fn hydrate_workspace_snapshot_lineage(
    workspace_root: &Path,
    snapshot: &mut WorkspaceSnapshot,
) -> Result<(), String> {
    snapshot.lineage = load_workspace_lineage(workspace_root, &snapshot.session_id)?;
    Ok(())
}

pub fn load_workspace_lineage(
    workspace_root: &Path,
    selected_session_id: &str,
) -> Result<Option<WorkspaceLineageSnapshot>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(None);
    };

    let selected_replay = load_replay_branch_link(&connection, selected_session_id)?;
    let anchor_candidate = selected_replay
        .as_ref()
        .and_then(|link| normalized_non_empty(link.source_session_id.as_deref()))
        .unwrap_or(selected_session_id);

    let Some(anchor_identity) = load_session_identity(&connection, anchor_candidate)? else {
        let selected_identity = load_session_identity(&connection, selected_session_id)?;
        return Ok(selected_identity.map(|identity| WorkspaceLineageSnapshot {
            anchor_session_id: identity.session_id.clone(),
            selected_session_id: identity.session_id.clone(),
            selected_kind: if selected_replay.is_some() {
                WorkspaceLineageBranchKind::Replay
            } else {
                WorkspaceLineageBranchKind::Base
            },
            branches: vec![WorkspaceLineageBranch {
                session_id: identity.session_id,
                kind: if selected_replay.is_some() {
                    WorkspaceLineageBranchKind::Replay
                } else {
                    WorkspaceLineageBranchKind::Base
                },
                title: identity.title,
                created_at_ms: identity.created_at_ms,
                active_pid: identity.active_pid,
                last_pid: identity.last_pid,
                source_dump_id: selected_replay.map(|link| link.source_dump_id),
                selected: true,
            }],
        }));
    };

    let mut branches = vec![WorkspaceLineageBranch {
        session_id: anchor_identity.session_id.clone(),
        kind: WorkspaceLineageBranchKind::Base,
        title: anchor_identity.title.clone(),
        created_at_ms: anchor_identity.created_at_ms,
        active_pid: anchor_identity.active_pid,
        last_pid: anchor_identity.last_pid,
        source_dump_id: None,
        selected: false,
    }];

    for descriptor in load_replay_branch_descriptors(&connection, &anchor_identity.session_id)? {
        let Some(identity) = load_session_identity(&connection, &descriptor.session_id)? else {
            continue;
        };
        branches.push(WorkspaceLineageBranch {
            session_id: identity.session_id,
            kind: WorkspaceLineageBranchKind::Replay,
            title: identity.title,
            created_at_ms: identity.created_at_ms,
            active_pid: identity.active_pid,
            last_pid: identity.last_pid,
            source_dump_id: Some(descriptor.source_dump_id),
            selected: false,
        });
    }

    branches.sort_by(|left, right| match (left.kind, right.kind) {
        (WorkspaceLineageBranchKind::Base, WorkspaceLineageBranchKind::Base) => {
            left.created_at_ms.cmp(&right.created_at_ms)
        }
        (WorkspaceLineageBranchKind::Base, _) => std::cmp::Ordering::Less,
        (_, WorkspaceLineageBranchKind::Base) => std::cmp::Ordering::Greater,
        _ => right.created_at_ms.cmp(&left.created_at_ms),
    });

    let selected_session_id = if branches
        .iter()
        .any(|branch| branch.session_id == selected_session_id)
    {
        selected_session_id.to_string()
    } else {
        anchor_identity.session_id.clone()
    };

    let selected_kind = branches
        .iter()
        .find(|branch| branch.session_id == selected_session_id)
        .map(|branch| branch.kind)
        .unwrap_or(WorkspaceLineageBranchKind::Base);

    for branch in &mut branches {
        branch.selected = branch.session_id == selected_session_id;
    }

    Ok(Some(WorkspaceLineageSnapshot {
        anchor_session_id: anchor_identity.session_id,
        selected_session_id,
        selected_kind,
        branches,
    }))
}

fn load_replay_branch_link(
    connection: &Connection,
    session_id: &str,
) -> Result<Option<ReplayBranchLink>, String> {
    if !table_exists(connection, "replay_branch_index")? {
        return Ok(None);
    }

    connection
        .query_row(
            r#"
            SELECT source_session_id, source_dump_id
            FROM replay_branch_index
            WHERE session_id = ?1
            "#,
            params![session_id],
            |row| {
                Ok(ReplayBranchLink {
                    source_session_id: row.get(0)?,
                    source_dump_id: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(|err| err.to_string())
}

fn load_replay_branch_descriptors(
    connection: &Connection,
    source_session_id: &str,
) -> Result<Vec<ReplayBranchDescriptor>, String> {
    if !table_exists(connection, "replay_branch_index")? {
        return Ok(Vec::new());
    }

    let mut statement = connection
        .prepare(
            r#"
            SELECT session_id, source_dump_id
            FROM replay_branch_index
            WHERE source_session_id = ?1
            ORDER BY created_at_ms DESC, session_id ASC
            "#,
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![source_session_id], |row| {
            Ok(ReplayBranchDescriptor {
                session_id: row.get(0)?,
                source_dump_id: row.get(1)?,
            })
        })
        .map_err(|err| err.to_string())?;

    let mut descriptors = Vec::new();
    for row in rows {
        descriptors.push(row.map_err(|err| err.to_string())?);
    }
    Ok(descriptors)
}

fn normalized_non_empty(value: Option<&str>) -> Option<&str> {
    value.and_then(|candidate| {
        let trimmed = candidate.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}
