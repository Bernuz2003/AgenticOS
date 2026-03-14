/// Kernel boot history and metadata storage.
use rusqlite::params;
use rusqlite::Connection;
use super::service::{StorageError, StorageService, current_timestamp_ms};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KernelBootRecord {
    pub(crate) boot_id: i64,
    pub(crate) started_at_ms: i64,
    pub(crate) kernel_version: String,
}

impl StorageService {
    pub(crate) fn record_kernel_boot(
        &mut self,
        kernel_version: &str,
    ) -> Result<KernelBootRecord, StorageError> {
        let started_at_ms = current_timestamp_ms();
        let transaction = self.connection.transaction()?;

        upsert_kernel_meta(
            &transaction,
            "kernel_version",
            kernel_version,
            started_at_ms,
        )?;
        upsert_kernel_meta(
            &transaction,
            "last_boot_started_at_ms",
            &started_at_ms.to_string(),
            started_at_ms,
        )?;
        transaction.execute(
            "INSERT INTO kernel_boots (started_at_ms, kernel_version) VALUES (?1, ?2)",
            params![started_at_ms, kernel_version],
        )?;
        let boot_id = transaction.last_insert_rowid();
        transaction.commit()?;

        Ok(KernelBootRecord {
            boot_id,
            started_at_ms,
            kernel_version: kernel_version.to_string(),
        })
    }
    #[cfg(test)]
    pub(crate) fn boot_count(&self) -> Result<i64, StorageError> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM kernel_boots", [], |row| row.get(0))?)
    }

    #[cfg(test)]
    pub(crate) fn meta_value(&self, key: &str) -> Result<Option<String>, StorageError> {
        use rusqlite::OptionalExtension;

        Ok(self
            .connection
            .query_row(
                "SELECT value FROM kernel_meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }
}

pub(super) fn upsert_kernel_meta(
    connection: &Connection,
    key: &str,
    value: &str,
    updated_at_ms: i64,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        r#"
        INSERT INTO kernel_meta (key, value, updated_at_ms)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at_ms = excluded.updated_at_ms
        "#,
        params![key, value, updated_at_ms],
    )?;

    Ok(())
}
