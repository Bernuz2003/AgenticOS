use super::migrations;
use agentic_control_models::DiagnosticEvent;
use rusqlite::params;
use rusqlite::Connection;
/// Core SQLite connection orchestration and configuration.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[cfg(test)]
#[path = "tests/service.rs"]
mod tests;

#[derive(Debug, Error)]
pub(crate) enum StorageError {
    #[error("failed to create storage directory '{path}': {source}")]
    CreateDir {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to open sqlite database '{path}': {source}")]
    Open {
        path: String,
        #[source]
        source: rusqlite::Error,
    },

    #[error("failed to configure sqlite database '{path}': {source}")]
    Configure {
        path: String,
        #[source]
        source: rusqlite::Error,
    },

    #[error("database schema version {found} is newer than supported version {supported}")]
    SchemaVersionTooNew { found: i32, supported: i32 },

    #[error("no active process run found for session '{session_id}' and pid {pid}")]
    MissingProcessRun { session_id: String, pid: u64 },

    #[error("turn {turn_id} not found")]
    MissingTurn { turn_id: i64 },

    #[error("kernel boot record is required before importing legacy timelines")]
    MissingKernelBoot,

    #[error("sqlite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub(crate) struct StorageService {
    path: PathBuf,
    pub(crate) connection: Connection,
    pending_diagnostics: Vec<DiagnosticEvent>,
}

impl StorageService {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        ensure_parent_dir(&path)?;

        let mut connection = Connection::open(&path).map_err(|source| StorageError::Open {
            path: path.display().to_string(),
            source,
        })?;
        configure_connection(&path, &connection)?;
        migrations::apply_pending_migrations(&mut connection)?;
        let mut service = Self {
            path,
            connection,
            pending_diagnostics: Vec::new(),
        };
        service.normalize_inline_assistant_thinking_once()?;

        Ok(service)
    }

    #[allow(dead_code)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn push_live_diagnostic(&mut self, event: DiagnosticEvent) {
        self.pending_diagnostics.push(event);
    }

    pub(crate) fn take_live_diagnostics(&mut self) -> Vec<DiagnosticEvent> {
        std::mem::take(&mut self.pending_diagnostics)
    }
}

impl StorageService {
    #[cfg(test)]
    pub(crate) fn schema_version(&self) -> Result<i32, StorageError> {
        Ok(self
            .connection
            .query_row("PRAGMA user_version;", [], |row| row.get(0))?)
    }

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

fn ensure_parent_dir(path: &Path) -> Result<(), StorageError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| StorageError::CreateDir {
            path: parent.display().to_string(),
            source,
        })?;
    }

    Ok(())
}

fn configure_connection(path: &Path, connection: &Connection) -> Result<(), StorageError> {
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|source| StorageError::Configure {
            path: path.display().to_string(),
            source,
        })?;
    connection
        .execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            PRAGMA synchronous = NORMAL;
            "#,
        )
        .map_err(|source| StorageError::Configure {
            path: path.display().to_string(),
            source,
        })?;

    Ok(())
}
pub(crate) fn current_timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KernelBootRecord {
    pub(crate) boot_id: i64,
    pub(crate) started_at_ms: i64,
    pub(crate) kernel_version: String,
}

pub(crate) fn upsert_kernel_meta(
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
