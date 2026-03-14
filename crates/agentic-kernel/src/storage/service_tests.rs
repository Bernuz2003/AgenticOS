/// Unit tests for core storage service and repositories.
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
