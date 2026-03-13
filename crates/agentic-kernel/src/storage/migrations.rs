use rusqlite::Connection;

use super::service::StorageError;

pub(crate) const LATEST_SCHEMA_VERSION: i32 = 7;

pub(super) fn apply_pending_migrations(connection: &mut Connection) -> Result<(), StorageError> {
    let current_version: i32 =
        connection.query_row("PRAGMA user_version;", [], |row| row.get(0))?;

    if current_version > LATEST_SCHEMA_VERSION {
        return Err(StorageError::SchemaVersionTooNew {
            found: current_version,
            supported: LATEST_SCHEMA_VERSION,
        });
    }

    if current_version < 1 {
        apply_v1_schema(connection)?;
    }
    if current_version < 2 {
        apply_v2_schema(connection)?;
    }
    if current_version < 3 {
        apply_v3_schema(connection)?;
    }
    if current_version < 4 {
        apply_v4_schema(connection)?;
    }
    if current_version < 5 {
        apply_v5_schema(connection)?;
    }
    if current_version < 6 {
        apply_v6_schema(connection)?;
    }
    if current_version < 7 {
        apply_v7_schema(connection)?;
    }

    Ok(())
}

fn apply_v1_schema(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;

    transaction.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS kernel_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS kernel_boots (
            boot_id INTEGER PRIMARY KEY AUTOINCREMENT,
            started_at_ms INTEGER NOT NULL,
            kernel_version TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_kernel_boots_started_at_ms
            ON kernel_boots(started_at_ms DESC);
        "#,
    )?;
    transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
    transaction.commit()?;

    Ok(())
}

fn apply_v2_schema(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;

    transaction.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
            session_id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            status TEXT NOT NULL,
            active_pid INTEGER NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_sessions_status
            ON sessions(status);

        CREATE TABLE IF NOT EXISTS process_runs (
            run_id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            boot_id INTEGER NOT NULL,
            pid INTEGER NOT NULL,
            state TEXT NOT NULL,
            started_at_ms INTEGER NOT NULL,
            ended_at_ms INTEGER NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE CASCADE,
            FOREIGN KEY(boot_id) REFERENCES kernel_boots(boot_id) ON DELETE CASCADE
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_process_runs_boot_pid
            ON process_runs(boot_id, pid);

        CREATE INDEX IF NOT EXISTS idx_process_runs_session_id
            ON process_runs(session_id, started_at_ms DESC);
        "#,
    )?;
    transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
    transaction.commit()?;

    Ok(())
}

fn apply_v3_schema(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;

    transaction.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS session_turns (
            turn_id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            pid INTEGER NOT NULL,
            turn_index INTEGER NOT NULL,
            workload TEXT NOT NULL,
            source TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            completed_at_ms INTEGER NULL,
            finish_reason TEXT NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_session_turns_session_turn_index
            ON session_turns(session_id, turn_index);

        CREATE INDEX IF NOT EXISTS idx_session_turns_session_started_at
            ON session_turns(session_id, started_at_ms ASC);

        CREATE INDEX IF NOT EXISTS idx_session_turns_pid
            ON session_turns(pid, started_at_ms DESC);

        CREATE TABLE IF NOT EXISTS session_messages (
            message_id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            turn_id INTEGER NOT NULL,
            pid INTEGER NOT NULL,
            ordinal INTEGER NOT NULL,
            role TEXT NOT NULL,
            kind TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE CASCADE,
            FOREIGN KEY(turn_id) REFERENCES session_turns(turn_id) ON DELETE CASCADE
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_session_messages_turn_ordinal
            ON session_messages(turn_id, ordinal);

        CREATE INDEX IF NOT EXISTS idx_session_messages_session
            ON session_messages(session_id, message_id ASC);
        "#,
    )?;
    transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
    transaction.commit()?;

    Ok(())
}

fn apply_v4_schema(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;

    transaction.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runtime_instances (
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

        CREATE INDEX IF NOT EXISTS idx_runtime_instances_state
            ON runtime_instances(state);

        CREATE INDEX IF NOT EXISTS idx_runtime_instances_last_used
            ON runtime_instances(last_used_at_ms DESC);

        ALTER TABLE sessions ADD COLUMN runtime_id TEXT NULL;
        CREATE INDEX IF NOT EXISTS idx_sessions_runtime_id
            ON sessions(runtime_id);

        ALTER TABLE process_runs ADD COLUMN runtime_id TEXT NULL;
        CREATE INDEX IF NOT EXISTS idx_process_runs_runtime_id
            ON process_runs(runtime_id, started_at_ms DESC);
        "#,
    )?;
    transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
    transaction.commit()?;

    Ok(())
}

fn apply_v5_schema(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;

    transaction.execute_batch(
        r#"
        ALTER TABLE runtime_instances
            ADD COLUMN reservation_ram_bytes INTEGER NOT NULL DEFAULT 0;

        ALTER TABLE runtime_instances
            ADD COLUMN reservation_vram_bytes INTEGER NOT NULL DEFAULT 0;

        ALTER TABLE runtime_instances
            ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;

        ALTER TABLE runtime_instances
            ADD COLUMN transition_state TEXT NULL;

        CREATE TABLE IF NOT EXISTS runtime_load_queue (
            queue_id INTEGER PRIMARY KEY AUTOINCREMENT,
            runtime_key TEXT NOT NULL,
            logical_model_id TEXT NOT NULL,
            display_path TEXT NOT NULL,
            backend_class TEXT NOT NULL,
            state TEXT NOT NULL,
            reservation_ram_bytes INTEGER NOT NULL,
            reservation_vram_bytes INTEGER NOT NULL,
            reason TEXT NOT NULL,
            requested_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_runtime_load_queue_state_requested
            ON runtime_load_queue(state, requested_at_ms ASC);

        CREATE INDEX IF NOT EXISTS idx_runtime_load_queue_runtime_key_state
            ON runtime_load_queue(runtime_key, state);
        "#,
    )?;
    transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
    transaction.commit()?;

    Ok(())
}

fn apply_v6_schema(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;

    transaction.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS accounting_events (
            event_id INTEGER PRIMARY KEY AUTOINCREMENT,
            recorded_at_ms INTEGER NOT NULL,
            session_id TEXT NULL,
            pid INTEGER NULL,
            runtime_id TEXT NULL,
            backend_id TEXT NOT NULL,
            backend_class TEXT NOT NULL,
            provider_id TEXT NULL,
            model_id TEXT NULL,
            request_kind TEXT NOT NULL,
            status TEXT NOT NULL,
            request_count INTEGER NOT NULL DEFAULT 0,
            stream INTEGER NOT NULL DEFAULT 0,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
            error_code TEXT NULL,
            error_message TEXT NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE SET NULL,
            FOREIGN KEY(runtime_id) REFERENCES runtime_instances(runtime_id) ON DELETE SET NULL
        );

        CREATE INDEX IF NOT EXISTS idx_accounting_events_recorded_at
            ON accounting_events(recorded_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_accounting_events_session_recorded
            ON accounting_events(session_id, recorded_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_accounting_events_runtime_recorded
            ON accounting_events(runtime_id, recorded_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_accounting_events_backend_recorded
            ON accounting_events(backend_id, recorded_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_accounting_events_provider_model
            ON accounting_events(provider_id, model_id, recorded_at_ms DESC);
        "#,
    )?;
    transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
    transaction.commit()?;

    Ok(())
}

fn apply_v7_schema(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;

    transaction.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS audit_events (
            audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
            recorded_at_ms INTEGER NOT NULL,
            category TEXT NOT NULL,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            detail TEXT NOT NULL,
            session_id TEXT NULL,
            pid INTEGER NULL,
            runtime_id TEXT NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE SET NULL,
            FOREIGN KEY(runtime_id) REFERENCES runtime_instances(runtime_id) ON DELETE SET NULL
        );

        CREATE INDEX IF NOT EXISTS idx_audit_events_recorded_at
            ON audit_events(recorded_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_audit_events_session_recorded
            ON audit_events(session_id, recorded_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_audit_events_pid_recorded
            ON audit_events(pid, recorded_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_audit_events_runtime_recorded
            ON audit_events(runtime_id, recorded_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_audit_events_category_kind_recorded
            ON audit_events(category, kind, recorded_at_ms DESC);
        "#,
    )?;
    transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
    transaction.commit()?;

    Ok(())
}
