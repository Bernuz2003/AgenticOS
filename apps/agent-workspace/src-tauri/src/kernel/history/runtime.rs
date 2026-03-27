use std::path::Path;

use agentic_control_models::{RuntimeInstanceView, RuntimeLoadQueueEntryView};
use rusqlite::Connection;

use super::db::{open_connection, table_exists};

pub fn load_runtime_instances(workspace_root: &Path) -> Result<Vec<RuntimeInstanceView>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(Vec::new());
    };
    load_runtime_instances_from_connection(&connection)
}

pub fn load_runtime_load_queue(
    workspace_root: &Path,
) -> Result<Vec<RuntimeLoadQueueEntryView>, String> {
    let Some(connection) = open_connection(workspace_root)? else {
        return Ok(Vec::new());
    };
    load_runtime_load_queue_from_connection(&connection)
}

pub(crate) fn load_runtime_instances_from_connection(
    connection: &Connection,
) -> Result<Vec<RuntimeInstanceView>, String> {
    if !table_exists(connection, "runtime_instances")? {
        return Ok(Vec::new());
    }

    let mut statement = connection
        .prepare(
            r#"
            SELECT
                runtime_id,
                target_kind,
                logical_model_id,
                display_path,
                family,
                backend_id,
                backend_class,
                provider_id,
                remote_model_id,
                state,
                reservation_ram_bytes,
                reservation_vram_bytes,
                pinned,
                transition_state,
                0 AS active_pid_count,
                '' AS active_pids_json,
                0 AS current
            FROM runtime_instances
            ORDER BY updated_at_ms DESC, runtime_id ASC
            "#,
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(RuntimeInstanceView {
                runtime_id: row.get(0)?,
                target_kind: row.get(1)?,
                logical_model_id: row.get(2)?,
                display_path: row.get(3)?,
                family: row.get(4)?,
                backend_id: row.get(5)?,
                backend_class: row.get(6)?,
                provider_id: row.get(7)?,
                remote_model_id: row.get(8)?,
                state: row.get(9)?,
                reservation_ram_bytes: row.get(10)?,
                reservation_vram_bytes: row.get(11)?,
                pinned: row.get::<_, i64>(12)? != 0,
                transition_state: row.get(13)?,
                active_pid_count: row.get::<_, i64>(14)? as usize,
                active_pids: Vec::new(),
                current: row.get::<_, i64>(16)? != 0,
            })
        })
        .map_err(|err| err.to_string())?;

    let mut runtimes = Vec::new();
    for row in rows {
        runtimes.push(row.map_err(|err| err.to_string())?);
    }
    Ok(runtimes)
}

pub(crate) fn load_runtime_load_queue_from_connection(
    connection: &Connection,
) -> Result<Vec<RuntimeLoadQueueEntryView>, String> {
    if !table_exists(connection, "runtime_load_queue")? {
        return Ok(Vec::new());
    }

    let mut statement = connection
        .prepare(
            r#"
            SELECT
                queue_id,
                logical_model_id,
                display_path,
                backend_class,
                state,
                reservation_ram_bytes,
                reservation_vram_bytes,
                reason,
                requested_at_ms,
                updated_at_ms
            FROM runtime_load_queue
            ORDER BY requested_at_ms DESC, queue_id DESC
            LIMIT 32
            "#,
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(RuntimeLoadQueueEntryView {
                queue_id: row.get(0)?,
                logical_model_id: row.get(1)?,
                display_path: row.get(2)?,
                backend_class: row.get(3)?,
                state: row.get(4)?,
                reservation_ram_bytes: row.get(5)?,
                reservation_vram_bytes: row.get(6)?,
                reason: row.get(7)?,
                requested_at_ms: row.get(8)?,
                updated_at_ms: row.get(9)?,
            })
        })
        .map_err(|err| err.to_string())?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row.map_err(|err| err.to_string())?);
    }
    Ok(entries)
}
