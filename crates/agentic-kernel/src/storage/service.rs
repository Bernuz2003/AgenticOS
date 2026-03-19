use super::migrations;
use agentic_control_models::DiagnosticEvent;
use rusqlite::Connection;
/// Core SQLite connection orchestration and configuration.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub(crate) use super::sessions_repo::StoredSessionRecord;

#[cfg(test)]
#[path = "service_tests.rs"]
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

    #[error("sqlite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub(crate) struct StorageService {
    path: PathBuf,
    pub(super) connection: Connection,
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

        Ok(Self {
            path,
            connection,
            pending_diagnostics: Vec::new(),
        })
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
