use rusqlite::Connection;

use super::service::StorageError;

pub(crate) const LATEST_SCHEMA_VERSION: i32 = 11;

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
    if current_version < 8 {
        apply_v8_schema(connection)?;
    }
    if current_version < 9 {
        apply_v9_schema(connection)?;
    }
    if current_version < 10 {
        apply_v10_schema(connection)?;
    }
    if current_version < 11 {
        apply_v11_schema(connection)?;
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

fn apply_v8_schema(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;

    transaction.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS workflow_task_attempts (
            orchestration_id INTEGER NOT NULL,
            task_id TEXT NOT NULL,
            attempt INTEGER NOT NULL,
            status TEXT NOT NULL,
            session_id TEXT NULL,
            pid INTEGER NULL,
            error TEXT NULL,
            output_preview TEXT NOT NULL DEFAULT '',
            output_chars INTEGER NOT NULL DEFAULT 0,
            truncated INTEGER NOT NULL DEFAULT 0,
            started_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            completed_at_ms INTEGER NULL,
            primary_artifact_id TEXT NULL,
            PRIMARY KEY(orchestration_id, task_id, attempt),
            FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE SET NULL
        );

        CREATE INDEX IF NOT EXISTS idx_workflow_task_attempts_orch_task
            ON workflow_task_attempts(orchestration_id, task_id, attempt DESC);

        CREATE INDEX IF NOT EXISTS idx_workflow_task_attempts_status
            ON workflow_task_attempts(orchestration_id, status, updated_at_ms DESC);

        CREATE TABLE IF NOT EXISTS workflow_artifacts (
            artifact_id TEXT PRIMARY KEY,
            orchestration_id INTEGER NOT NULL,
            producer_task_id TEXT NOT NULL,
            producer_attempt INTEGER NOT NULL,
            kind TEXT NOT NULL,
            label TEXT NOT NULL,
            mime_type TEXT NOT NULL,
            content_text TEXT NOT NULL,
            preview TEXT NOT NULL,
            bytes INTEGER NOT NULL,
            created_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_workflow_artifacts_orch_task
            ON workflow_artifacts(orchestration_id, producer_task_id, producer_attempt DESC);

        CREATE TABLE IF NOT EXISTS workflow_task_artifact_inputs (
            orchestration_id INTEGER NOT NULL,
            consumer_task_id TEXT NOT NULL,
            consumer_attempt INTEGER NOT NULL,
            artifact_id TEXT NOT NULL,
            producer_task_id TEXT NOT NULL,
            producer_attempt INTEGER NOT NULL,
            PRIMARY KEY(orchestration_id, consumer_task_id, consumer_attempt, artifact_id),
            FOREIGN KEY(artifact_id) REFERENCES workflow_artifacts(artifact_id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_workflow_task_artifact_inputs_consumer
            ON workflow_task_artifact_inputs(
                orchestration_id,
                consumer_task_id,
                consumer_attempt
            );
        "#,
    )?;
    transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
    transaction.commit()?;

    Ok(())
}

fn apply_v9_schema(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;

    transaction.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS scheduled_jobs (
            job_id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            target_kind TEXT NOT NULL,
            workflow_payload TEXT NOT NULL,
            trigger_kind TEXT NOT NULL,
            trigger_payload TEXT NOT NULL,
            timeout_ms INTEGER NOT NULL,
            max_retries INTEGER NOT NULL,
            backoff_ms INTEGER NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            state TEXT NOT NULL,
            next_run_at_ms INTEGER NULL,
            current_trigger_at_ms INTEGER NULL,
            current_attempt INTEGER NOT NULL DEFAULT 0,
            active_run_id INTEGER NULL,
            active_orchestration_id INTEGER NULL,
            active_deadline_at_ms INTEGER NULL,
            last_run_started_at_ms INTEGER NULL,
            last_run_completed_at_ms INTEGER NULL,
            last_run_status TEXT NULL,
            last_error TEXT NULL,
            consecutive_failures INTEGER NOT NULL DEFAULT 0,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_next_run
            ON scheduled_jobs(enabled, next_run_at_ms);

        CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_active_orch
            ON scheduled_jobs(active_orchestration_id);

        CREATE TABLE IF NOT EXISTS scheduled_job_runs (
            run_id INTEGER PRIMARY KEY AUTOINCREMENT,
            job_id INTEGER NOT NULL,
            trigger_at_ms INTEGER NOT NULL,
            attempt INTEGER NOT NULL,
            status TEXT NOT NULL,
            started_at_ms INTEGER NULL,
            completed_at_ms INTEGER NULL,
            orchestration_id INTEGER NULL,
            deadline_at_ms INTEGER NULL,
            error TEXT NULL,
            FOREIGN KEY(job_id) REFERENCES scheduled_jobs(job_id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_scheduled_job_runs_job_id
            ON scheduled_job_runs(job_id, run_id DESC);

        CREATE INDEX IF NOT EXISTS idx_scheduled_job_runs_orch
            ON scheduled_job_runs(orchestration_id);
        "#,
    )?;
    transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
    transaction.commit()?;

    Ok(())
}

fn apply_v10_schema(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;

    transaction.execute_batch(
        r#"
        ALTER TABLE workflow_task_attempts
            ADD COLUMN termination_reason TEXT NULL;

        CREATE TABLE IF NOT EXISTS ipc_messages (
            message_id TEXT PRIMARY KEY,
            orchestration_id INTEGER NULL,
            sender_pid INTEGER NULL,
            sender_task_id TEXT NULL,
            sender_attempt INTEGER NULL,
            receiver_pid INTEGER NULL,
            receiver_task_id TEXT NULL,
            receiver_attempt INTEGER NULL,
            message_type TEXT NOT NULL,
            channel TEXT NULL,
            payload_preview TEXT NOT NULL,
            payload_text TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            delivered_at_ms INTEGER NULL,
            consumed_at_ms INTEGER NULL
        );

        CREATE INDEX IF NOT EXISTS idx_ipc_messages_orch_created
            ON ipc_messages(orchestration_id, created_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_ipc_messages_sender_created
            ON ipc_messages(sender_pid, created_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_ipc_messages_receiver_created
            ON ipc_messages(receiver_pid, created_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_ipc_messages_status_created
            ON ipc_messages(status, created_at_ms DESC);
        "#,
    )?;
    transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
    transaction.commit()?;

    Ok(())
}

fn apply_v11_schema(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;

    transaction.execute_batch(
        r#"
        ALTER TABLE ipc_messages
            ADD COLUMN receiver_role TEXT NULL;

        ALTER TABLE ipc_messages
            ADD COLUMN failed_at_ms INTEGER NULL;

        CREATE INDEX IF NOT EXISTS idx_ipc_messages_task_created
            ON ipc_messages(orchestration_id, receiver_task_id, created_at_ms ASC);

        CREATE INDEX IF NOT EXISTS idx_ipc_messages_role_created
            ON ipc_messages(orchestration_id, receiver_role, created_at_ms ASC);

        CREATE INDEX IF NOT EXISTS idx_ipc_messages_channel_created
            ON ipc_messages(orchestration_id, channel, created_at_ms ASC);
        "#,
    )?;
    transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
    transaction.commit()?;

    Ok(())
}
