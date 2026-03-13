use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use thiserror::Error;

use super::migrations;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KernelBootRecord {
    pub(crate) boot_id: i64,
    pub(crate) started_at_ms: i64,
    pub(crate) kernel_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredSessionRecord {
    pub(crate) session_id: String,
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) runtime_id: Option<String>,
    pub(crate) active_pid: Option<u64>,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
}

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

        Ok(Self { path, connection })
    }

    #[allow(dead_code)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
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

    pub(crate) fn reset_active_sessions_for_boot(&mut self) -> Result<usize, StorageError> {
        Ok(self.connection.execute(
            r#"
            UPDATE sessions
            SET
                active_pid = NULL,
                status = CASE
                    WHEN status = 'running' THEN 'idle'
                    ELSE status
                END,
                updated_at_ms = ?1
            WHERE active_pid IS NOT NULL OR status = 'running'
            "#,
            params![current_timestamp_ms()],
        )?)
    }

    pub(crate) fn load_sessions(&self) -> Result<Vec<StoredSessionRecord>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT session_id, title, status, runtime_id, active_pid, created_at_ms, updated_at_ms
            FROM sessions
            ORDER BY created_at_ms ASC
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredSessionRecord {
                session_id: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                runtime_id: row.get(3)?,
                active_pid: row.get(4)?,
                created_at_ms: row.get(5)?,
                updated_at_ms: row.get(6)?,
            })
        })?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }

        Ok(sessions)
    }

    pub(crate) fn insert_session(
        &mut self,
        session_id: &str,
        title: &str,
        status: &str,
        runtime_id: Option<&str>,
        active_pid: Option<u64>,
        created_at_ms: i64,
        updated_at_ms: i64,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            r#"
            INSERT INTO sessions (
                session_id,
                title,
                status,
                runtime_id,
                active_pid,
                created_at_ms,
                updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                session_id,
                title,
                status,
                runtime_id,
                active_pid,
                created_at_ms,
                updated_at_ms
            ],
        )?;

        Ok(())
    }

    pub(crate) fn delete_session(&mut self, session_id: &str) -> Result<(), StorageError> {
        self.connection.execute(
            "DELETE FROM sessions WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    pub(crate) fn bind_session_to_pid(
        &mut self,
        session_id: &str,
        runtime_id: &str,
        boot_id: i64,
        pid: u64,
        started_at_ms: i64,
    ) -> Result<(), StorageError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            r#"
            UPDATE sessions
            SET active_pid = ?2, runtime_id = ?3, status = 'running', updated_at_ms = ?4
            WHERE session_id = ?1
            "#,
            params![session_id, pid, runtime_id, started_at_ms],
        )?;
        transaction.execute(
            r#"
            INSERT INTO process_runs (
                session_id,
                boot_id,
                pid,
                runtime_id,
                state,
                started_at_ms,
                ended_at_ms
            ) VALUES (?1, ?2, ?3, ?4, 'running', ?5, NULL)
            "#,
            params![session_id, boot_id, pid, runtime_id, started_at_ms],
        )?;
        transaction.commit()?;

        Ok(())
    }

    pub(crate) fn release_session_pid(
        &mut self,
        session_id: &str,
        boot_id: i64,
        pid: u64,
        run_state: &str,
        ended_at_ms: i64,
    ) -> Result<(), StorageError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            r#"
            UPDATE sessions
            SET active_pid = NULL, status = 'idle', updated_at_ms = ?2
            WHERE session_id = ?1 AND active_pid = ?4
            "#,
            params![session_id, ended_at_ms, pid],
        )?;
        transaction.execute(
            r#"
            UPDATE process_runs
            SET state = ?4, ended_at_ms = ?5
            WHERE session_id = ?1 AND boot_id = ?2 AND pid = ?3
            "#,
            params![session_id, boot_id, pid, run_state, ended_at_ms],
        )?;
        transaction.commit()?;

        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn schema_version(&self) -> Result<i32, StorageError> {
        Ok(self
            .connection
            .query_row("PRAGMA user_version;", [], |row| row.get(0))?)
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

    #[cfg(test)]
    pub(crate) fn session_by_id(
        &self,
        session_id: &str,
    ) -> Result<Option<StoredSessionRecord>, StorageError> {
        use rusqlite::OptionalExtension;

        Ok(self
            .connection
            .query_row(
                r#"
                SELECT session_id, title, status, runtime_id, active_pid, created_at_ms, updated_at_ms
                FROM sessions
                WHERE session_id = ?1
                "#,
                params![session_id],
                |row| {
                    Ok(StoredSessionRecord {
                        session_id: row.get(0)?,
                        title: row.get(1)?,
                        status: row.get(2)?,
                        runtime_id: row.get(3)?,
                        active_pid: row.get(4)?,
                        created_at_ms: row.get(5)?,
                        updated_at_ms: row.get(6)?,
                    })
                },
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

pub(super) fn current_timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::StorageService;
    use crate::storage::migrations::LATEST_SCHEMA_VERSION;
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn open_initializes_schema_and_persists_boot_metadata() {
        let dir = make_temp_dir("agenticos_storage_bootstrap");
        let db_path = dir.join("agenticos.db");

        let mut storage = StorageService::open(&db_path).expect("open storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record kernel boot");

        assert_eq!(
            storage.schema_version().expect("schema version"),
            LATEST_SCHEMA_VERSION
        );
        assert_eq!(boot.boot_id, 1);
        assert_eq!(storage.boot_count().expect("boot count"), 1);
        assert_eq!(
            storage
                .meta_value("kernel_version")
                .expect("kernel version"),
            Some("0.5.0-test".to_string())
        );
        assert_eq!(
            storage
                .meta_value("last_boot_started_at_ms")
                .expect("last boot timestamp")
                .is_some(),
            true
        );
        assert!(db_path.exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn reopen_keeps_schema_and_appends_boot_history() {
        let dir = make_temp_dir("agenticos_storage_reopen");
        let db_path = dir.join("agenticos.db");

        {
            let mut storage = StorageService::open(&db_path).expect("open storage");
            storage
                .record_kernel_boot("0.5.0-first")
                .expect("record first boot");
        }

        let mut reopened = StorageService::open(&db_path).expect("reopen storage");
        let boot = reopened
            .record_kernel_boot("0.5.0-second")
            .expect("record second boot");

        assert_eq!(boot.boot_id, 2);
        assert_eq!(reopened.boot_count().expect("boot count"), 2);
        assert_eq!(
            reopened
                .meta_value("kernel_version")
                .expect("kernel version"),
            Some("0.5.0-second".to_string())
        );
        assert_eq!(
            reopened.schema_version().expect("schema version"),
            LATEST_SCHEMA_VERSION
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn session_records_survive_reopen_and_active_pid_can_be_reset() {
        let dir = make_temp_dir("agenticos_storage_sessions");
        let db_path = dir.join("agenticos.db");

        let mut storage = StorageService::open(&db_path).expect("open storage");
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .expect("record kernel boot");
        storage
            .insert_session(
                "sess-1-000001",
                "hello session",
                "idle",
                Some("rt-test"),
                None,
                1_000,
                1_000,
            )
            .expect("insert session");
        storage
            .bind_session_to_pid("sess-1-000001", "rt-test", boot.boot_id, 7, 2_000)
            .expect("bind session");

        drop(storage);

        let mut reopened = StorageService::open(&db_path).expect("reopen storage");
        assert_eq!(
            reopened
                .session_by_id("sess-1-000001")
                .expect("load session")
                .expect("session exists")
                .active_pid,
            Some(7)
        );

        reopened
            .reset_active_sessions_for_boot()
            .expect("reset active sessions");
        let session = reopened
            .session_by_id("sess-1-000001")
            .expect("load session")
            .expect("session exists");
        assert_eq!(session.active_pid, None);
        assert_eq!(session.status, "idle");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_v4_schema_is_migrated_to_latest_version() {
        let dir = make_temp_dir("agenticos_storage_legacy_v4");
        let db_path = dir.join("agenticos.db");

        {
            let connection = Connection::open(&db_path).expect("open legacy db");
            connection
                .execute_batch(
                    r#"
                    CREATE TABLE kernel_meta (
                        key TEXT PRIMARY KEY,
                        value TEXT NOT NULL,
                        updated_at_ms INTEGER NOT NULL
                    );

                    CREATE TABLE kernel_boots (
                        boot_id INTEGER PRIMARY KEY AUTOINCREMENT,
                        started_at_ms INTEGER NOT NULL,
                        kernel_version TEXT NOT NULL
                    );

                    CREATE TABLE sessions (
                        session_id TEXT PRIMARY KEY,
                        title TEXT NOT NULL,
                        status TEXT NOT NULL,
                        active_pid INTEGER NULL,
                        created_at_ms INTEGER NOT NULL,
                        updated_at_ms INTEGER NOT NULL,
                        runtime_id TEXT NULL
                    );

                    CREATE TABLE process_runs (
                        run_id INTEGER PRIMARY KEY AUTOINCREMENT,
                        session_id TEXT NOT NULL,
                        boot_id INTEGER NOT NULL,
                        pid INTEGER NOT NULL,
                        state TEXT NOT NULL,
                        started_at_ms INTEGER NOT NULL,
                        ended_at_ms INTEGER NULL,
                        runtime_id TEXT NULL
                    );

                    CREATE TABLE runtime_instances (
                        runtime_id TEXT PRIMARY KEY,
                        runtime_key TEXT NOT NULL UNIQUE,
                        state TEXT NOT NULL,
                        target_kind TEXT NOT NULL,
                        logical_model_id TEXT NOT NULL,
                        display_path TEXT NOT NULL,
                        runtime_reference TEXT NOT NULL,
                        family TEXT NOT NULL,
                        backend_id TEXT NOT NULL,
                        backend_class TEXT NOT NULL,
                        driver_source TEXT NOT NULL,
                        driver_rationale TEXT NOT NULL,
                        provider_id TEXT NULL,
                        remote_model_id TEXT NULL,
                        load_mode TEXT NOT NULL,
                        created_at_ms INTEGER NOT NULL,
                        updated_at_ms INTEGER NOT NULL,
                        last_used_at_ms INTEGER NOT NULL
                    );

                    PRAGMA user_version = 4;
                    "#,
                )
                .expect("create legacy schema");
            connection
                .execute(
                    "INSERT INTO sessions (session_id, title, status, active_pid, created_at_ms, updated_at_ms, runtime_id) VALUES ('sess-legacy', 'Legacy session', 'idle', NULL, 10, 10, 'rt-legacy')",
                    [],
                )
                .expect("insert legacy session");
            connection
                .execute(
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
                        created_at_ms,
                        updated_at_ms,
                        last_used_at_ms
                    ) VALUES (
                        'rt-legacy',
                        'local::legacy',
                        'ready',
                        'local',
                        'qwen2',
                        '/models/qwen2.gguf',
                        '/models/qwen2.gguf',
                        'qwen',
                        'llamacpp',
                        'local_resident',
                        'local_catalog',
                        'legacy fixture',
                        NULL,
                        NULL,
                        'resident',
                        10,
                        10,
                        10
                    )
                    "#,
                    [],
                )
                .expect("insert legacy runtime");
        }

        let storage = StorageService::open(&db_path).expect("migrate storage");
        assert_eq!(
            storage.schema_version().expect("schema version"),
            LATEST_SCHEMA_VERSION
        );
        assert_eq!(
            storage
                .session_by_id("sess-legacy")
                .expect("load legacy session")
                .expect("legacy session exists")
                .runtime_id
                .as_deref(),
            Some("rt-legacy")
        );
        assert!(table_has_column(
            &storage.connection,
            "runtime_instances",
            "reservation_ram_bytes"
        ));
        assert!(table_has_column(
            &storage.connection,
            "runtime_instances",
            "reservation_vram_bytes"
        ));
        assert!(table_has_column(
            &storage.connection,
            "runtime_instances",
            "pinned"
        ));
        assert!(table_has_column(
            &storage.connection,
            "runtime_instances",
            "transition_state"
        ));
        assert!(table_exists(&storage.connection, "runtime_load_queue"));
        assert!(table_exists(&storage.connection, "accounting_events"));
        assert!(table_exists(&storage.connection, "audit_events"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn writer_restart_rolls_back_inflight_transaction_and_preserves_committed_rows() {
        let dir = make_temp_dir("agenticos_storage_writer_recovery");
        let db_path = dir.join("agenticos.db");

        {
            let mut storage = StorageService::open(&db_path).expect("open storage");
            storage
                .insert_session("sess-committed", "Committed", "idle", None, None, 1, 1)
                .expect("insert committed session");
        }

        {
            let mut connection = Connection::open(&db_path).expect("open raw writer");
            connection
                .pragma_update(None, "journal_mode", "WAL")
                .expect("set wal");
            let transaction = connection.transaction().expect("begin transaction");
            transaction
                .execute(
                    "INSERT INTO sessions (session_id, title, status, active_pid, created_at_ms, updated_at_ms, runtime_id) VALUES ('sess-inflight', 'Inflight', 'running', 77, 2, 2, NULL)",
                    [],
                )
                .expect("insert inflight session");
        }

        let reopened = StorageService::open(&db_path).expect("reopen storage");
        assert_eq!(
            reopened
                .session_by_id("sess-committed")
                .expect("load committed session")
                .expect("committed session exists")
                .title,
            "Committed"
        );
        assert!(reopened
            .session_by_id("sess-inflight")
            .expect("load inflight session")
            .is_none());

        let _ = fs::remove_dir_all(dir);
    }

    fn table_exists(connection: &Connection, table: &str) -> bool {
        connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |_row| Ok(()),
            )
            .is_ok()
    }

    fn table_has_column(connection: &Connection, table: &str, column: &str) -> bool {
        let pragma = format!("PRAGMA table_info({table})");
        let mut statement = connection.prepare(&pragma).expect("prepare pragma");
        let rows = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query table info");
        let present = rows.filter_map(Result::ok).any(|name| name == column);
        present
    }

    fn make_temp_dir(prefix: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), timestamp));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
