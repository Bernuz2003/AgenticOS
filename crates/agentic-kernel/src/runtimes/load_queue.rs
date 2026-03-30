use rusqlite::params;

use crate::storage::{current_timestamp_ms, StorageError, StorageService};
use agentic_control_models::RuntimeLoadQueueEntryView;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredRuntimeLoadQueueEntry {
    pub(crate) queue_id: i64,
    pub(crate) runtime_key: String,
    pub(crate) logical_model_id: String,
    pub(crate) display_path: String,
    pub(crate) backend_class: String,
    pub(crate) state: String,
    pub(crate) reservation_ram_bytes: u64,
    pub(crate) reservation_vram_bytes: u64,
    pub(crate) reason: String,
    pub(crate) requested_at_ms: i64,
    pub(crate) updated_at_ms: i64,
}

pub(crate) fn build_runtime_load_queue_views(
    entries: Vec<crate::resource_governor::RuntimeLoadQueueEntry>,
) -> Vec<RuntimeLoadQueueEntryView> {
    entries
        .into_iter()
        .map(|entry| RuntimeLoadQueueEntryView {
            queue_id: entry.queue_id,
            logical_model_id: entry.logical_model_id,
            display_path: entry.display_path,
            backend_class: entry.backend_class,
            state: entry.state,
            reservation_ram_bytes: entry.reservation_ram_bytes,
            reservation_vram_bytes: entry.reservation_vram_bytes,
            reason: entry.reason,
            requested_at_ms: entry.requested_at_ms,
            updated_at_ms: entry.updated_at_ms,
        })
        .collect()
}

impl StorageService {
    pub(crate) fn load_runtime_load_queue_entries(
        &self,
        limit: usize,
    ) -> Result<Vec<StoredRuntimeLoadQueueEntry>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                queue_id,
                runtime_key,
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
            LIMIT ?1
            "#,
        )?;
        let rows = statement.query_map(params![limit as i64], |row| {
            Ok(StoredRuntimeLoadQueueEntry {
                queue_id: row.get(0)?,
                runtime_key: row.get(1)?,
                logical_model_id: row.get(2)?,
                display_path: row.get(3)?,
                backend_class: row.get(4)?,
                state: row.get(5)?,
                reservation_ram_bytes: row.get(6)?,
                reservation_vram_bytes: row.get(7)?,
                reason: row.get(8)?,
                requested_at_ms: row.get(9)?,
                updated_at_ms: row.get(10)?,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }

        Ok(entries)
    }

    pub(crate) fn find_pending_runtime_load_queue_entry(
        &self,
        runtime_key: &str,
    ) -> Result<Option<StoredRuntimeLoadQueueEntry>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                queue_id,
                runtime_key,
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
            WHERE runtime_key = ?1 AND state = 'pending'
            ORDER BY queue_id DESC
            LIMIT 1
            "#,
        )?;
        let mut rows = statement.query(params![runtime_key])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };

        Ok(Some(StoredRuntimeLoadQueueEntry {
            queue_id: row.get(0)?,
            runtime_key: row.get(1)?,
            logical_model_id: row.get(2)?,
            display_path: row.get(3)?,
            backend_class: row.get(4)?,
            state: row.get(5)?,
            reservation_ram_bytes: row.get(6)?,
            reservation_vram_bytes: row.get(7)?,
            reason: row.get(8)?,
            requested_at_ms: row.get(9)?,
            updated_at_ms: row.get(10)?,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn insert_runtime_load_queue_entry(
        &mut self,
        runtime_key: &str,
        logical_model_id: &str,
        display_path: &str,
        backend_class: &str,
        state: &str,
        reservation_ram_bytes: u64,
        reservation_vram_bytes: u64,
        reason: &str,
    ) -> Result<StoredRuntimeLoadQueueEntry, StorageError> {
        let now = current_timestamp_ms();
        self.connection.execute(
            r#"
            INSERT INTO runtime_load_queue (
                runtime_key,
                logical_model_id,
                display_path,
                backend_class,
                state,
                reservation_ram_bytes,
                reservation_vram_bytes,
                reason,
                requested_at_ms,
                updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                runtime_key,
                logical_model_id,
                display_path,
                backend_class,
                state,
                reservation_ram_bytes,
                reservation_vram_bytes,
                reason,
                now,
                now,
            ],
        )?;
        let queue_id = self.connection.last_insert_rowid();

        Ok(StoredRuntimeLoadQueueEntry {
            queue_id,
            runtime_key: runtime_key.to_string(),
            logical_model_id: logical_model_id.to_string(),
            display_path: display_path.to_string(),
            backend_class: backend_class.to_string(),
            state: state.to_string(),
            reservation_ram_bytes,
            reservation_vram_bytes,
            reason: reason.to_string(),
            requested_at_ms: now,
            updated_at_ms: now,
        })
    }

    pub(crate) fn update_runtime_load_queue_entry(
        &mut self,
        queue_id: i64,
        state: &str,
        reservation_ram_bytes: u64,
        reservation_vram_bytes: u64,
        reason: &str,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            r#"
            UPDATE runtime_load_queue
            SET
                state = ?2,
                reservation_ram_bytes = ?3,
                reservation_vram_bytes = ?4,
                reason = ?5,
                updated_at_ms = ?6
            WHERE queue_id = ?1
            "#,
            params![
                queue_id,
                state,
                reservation_ram_bytes,
                reservation_vram_bytes,
                reason,
                current_timestamp_ms(),
            ],
        )?;
        Ok(())
    }

    pub(crate) fn mark_runtime_load_queue_entries_for_runtime(
        &mut self,
        runtime_key: &str,
        from_state: &str,
        to_state: &str,
        reason: &str,
    ) -> Result<usize, StorageError> {
        Ok(self.connection.execute(
            r#"
            UPDATE runtime_load_queue
            SET
                state = ?3,
                reason = ?4,
                updated_at_ms = ?5
            WHERE runtime_key = ?1 AND state = ?2
            "#,
            params![
                runtime_key,
                from_state,
                to_state,
                reason,
                current_timestamp_ms(),
            ],
        )?)
    }
}
