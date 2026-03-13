use rusqlite::params;

use super::service::{current_timestamp_ms, StorageError, StorageService};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredRuntimeRecord {
    pub(crate) runtime_id: String,
    pub(crate) runtime_key: String,
    pub(crate) state: String,
    pub(crate) target_kind: String,
    pub(crate) logical_model_id: String,
    pub(crate) display_path: String,
    pub(crate) runtime_reference: String,
    pub(crate) family: String,
    pub(crate) backend_id: String,
    pub(crate) backend_class: String,
    pub(crate) driver_source: String,
    pub(crate) driver_rationale: String,
    pub(crate) provider_id: Option<String>,
    pub(crate) remote_model_id: Option<String>,
    pub(crate) load_mode: String,
    pub(crate) reservation_ram_bytes: u64,
    pub(crate) reservation_vram_bytes: u64,
    pub(crate) pinned: bool,
    pub(crate) transition_state: Option<String>,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) last_used_at_ms: i64,
}

impl StorageService {
    pub(crate) fn reset_runtime_instances_for_boot(&mut self) -> Result<usize, StorageError> {
        Ok(self.connection.execute(
            r#"
            UPDATE runtime_instances
            SET
                state = 'registered',
                transition_state = NULL,
                updated_at_ms = ?1
            WHERE state != 'registered'
            "#,
            params![current_timestamp_ms()],
        )?)
    }

    pub(crate) fn load_runtime_instances(&self) -> Result<Vec<StoredRuntimeRecord>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                runtime_id,
                runtime_key,
                state,
                target_kind,
                logical_model_id,
                display_path,
                runtime_reference,
                family,
                backend_id,
                backend_class,
                driver_source,
                driver_rationale,
                provider_id,
                remote_model_id,
                load_mode,
                reservation_ram_bytes,
                reservation_vram_bytes,
                pinned,
                transition_state,
                created_at_ms,
                updated_at_ms,
                last_used_at_ms
            FROM runtime_instances
            ORDER BY created_at_ms ASC
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredRuntimeRecord {
                runtime_id: row.get(0)?,
                runtime_key: row.get(1)?,
                state: row.get(2)?,
                target_kind: row.get(3)?,
                logical_model_id: row.get(4)?,
                display_path: row.get(5)?,
                runtime_reference: row.get(6)?,
                family: row.get(7)?,
                backend_id: row.get(8)?,
                backend_class: row.get(9)?,
                driver_source: row.get(10)?,
                driver_rationale: row.get(11)?,
                provider_id: row.get(12)?,
                remote_model_id: row.get(13)?,
                load_mode: row.get(14)?,
                reservation_ram_bytes: row.get(15)?,
                reservation_vram_bytes: row.get(16)?,
                pinned: row.get::<_, i64>(17)? != 0,
                transition_state: row.get(18)?,
                created_at_ms: row.get(19)?,
                updated_at_ms: row.get(20)?,
                last_used_at_ms: row.get(21)?,
            })
        })?;

        let mut runtimes = Vec::new();
        for row in rows {
            runtimes.push(row?);
        }

        Ok(runtimes)
    }

    pub(crate) fn upsert_runtime_instance(
        &mut self,
        record: &StoredRuntimeRecord,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            r#"
            INSERT INTO runtime_instances (
                runtime_id,
                runtime_key,
                state,
                target_kind,
                logical_model_id,
                display_path,
                runtime_reference,
                family,
                backend_id,
                backend_class,
                driver_source,
                driver_rationale,
                provider_id,
                remote_model_id,
                load_mode,
                reservation_ram_bytes,
                reservation_vram_bytes,
                pinned,
                transition_state,
                created_at_ms,
                updated_at_ms,
                last_used_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
            ON CONFLICT(runtime_id) DO UPDATE SET
                runtime_key = excluded.runtime_key,
                state = excluded.state,
                target_kind = excluded.target_kind,
                logical_model_id = excluded.logical_model_id,
                display_path = excluded.display_path,
                runtime_reference = excluded.runtime_reference,
                family = excluded.family,
                backend_id = excluded.backend_id,
                backend_class = excluded.backend_class,
                driver_source = excluded.driver_source,
                driver_rationale = excluded.driver_rationale,
                provider_id = excluded.provider_id,
                remote_model_id = excluded.remote_model_id,
                load_mode = excluded.load_mode,
                reservation_ram_bytes = excluded.reservation_ram_bytes,
                reservation_vram_bytes = excluded.reservation_vram_bytes,
                pinned = excluded.pinned,
                transition_state = excluded.transition_state,
                updated_at_ms = excluded.updated_at_ms,
                last_used_at_ms = excluded.last_used_at_ms
            "#,
            params![
                record.runtime_id,
                record.runtime_key,
                record.state,
                record.target_kind,
                record.logical_model_id,
                record.display_path,
                record.runtime_reference,
                record.family,
                record.backend_id,
                record.backend_class,
                record.driver_source,
                record.driver_rationale,
                record.provider_id,
                record.remote_model_id,
                record.load_mode,
                record.reservation_ram_bytes,
                record.reservation_vram_bytes,
                if record.pinned { 1 } else { 0 },
                record.transition_state,
                record.created_at_ms,
                record.updated_at_ms,
                record.last_used_at_ms,
            ],
        )?;

        Ok(())
    }
}
