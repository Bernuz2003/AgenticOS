use rusqlite::{Connection, OptionalExtension, Transaction};

use super::service::StorageError;

pub(crate) const LATEST_SCHEMA_VERSION: i32 = 12;

const LEGACY_TABLES: &[&str] = &[
    "kernel_meta",
    "kernel_boots",
    "sessions",
    "process_runs",
    "session_turns",
    "session_messages",
    "runtime_instances",
    "runtime_load_queue",
    "accounting_events",
    "audit_events",
    "workflow_task_attempts",
    "workflow_artifacts",
    "workflow_task_artifact_inputs",
    "scheduled_jobs",
    "scheduled_job_runs",
    "ipc_messages",
];

pub(super) fn apply_pending_migrations(connection: &mut Connection) -> Result<(), StorageError> {
    let current_version: i32 =
        connection.query_row("PRAGMA user_version;", [], |row| row.get(0))?;

    if current_version > LATEST_SCHEMA_VERSION {
        return Err(StorageError::SchemaVersionTooNew {
            found: current_version,
            supported: LATEST_SCHEMA_VERSION,
        });
    }

    if current_version == LATEST_SCHEMA_VERSION {
        return Ok(());
    }

    if current_version == 0 {
        apply_baseline_schema(connection)?;
    } else {
        rebaseline_existing_schema(connection)?;
    }

    Ok(())
}

fn apply_baseline_schema(connection: &mut Connection) -> Result<(), StorageError> {
    let transaction = connection.transaction()?;
    create_baseline_schema(&transaction)?;
    transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(())
}

fn rebaseline_existing_schema(connection: &mut Connection) -> Result<(), StorageError> {
    connection.execute_batch("PRAGMA foreign_keys = OFF;")?;
    let transaction = connection.transaction()?;

    for table in LEGACY_TABLES {
        let legacy = legacy_table_name(table);
        if table_exists(&transaction, table)? {
            transaction.execute(
                &format!("ALTER TABLE {table} RENAME TO {legacy}"),
                [],
            )?;
        }
    }

    create_baseline_schema(&transaction)?;
    copy_legacy_rows(&transaction)?;
    drop_legacy_tables(&transaction)?;
    transaction.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
    transaction.commit()?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(())
}

fn create_baseline_schema(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    transaction.execute_batch(
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

        CREATE INDEX idx_kernel_boots_started_at_ms
            ON kernel_boots(started_at_ms DESC);

        CREATE TABLE sessions (
            session_id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            status TEXT NOT NULL,
            runtime_id TEXT NULL,
            active_pid INTEGER NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE INDEX idx_sessions_status ON sessions(status);
        CREATE INDEX idx_sessions_runtime_id ON sessions(runtime_id);

        CREATE TABLE process_runs (
            run_id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            boot_id INTEGER NOT NULL,
            pid INTEGER NOT NULL,
            runtime_id TEXT NULL,
            state TEXT NOT NULL,
            started_at_ms INTEGER NOT NULL,
            ended_at_ms INTEGER NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE CASCADE,
            FOREIGN KEY(boot_id) REFERENCES kernel_boots(boot_id) ON DELETE CASCADE
        );

        CREATE UNIQUE INDEX idx_process_runs_boot_pid
            ON process_runs(boot_id, pid);
        CREATE INDEX idx_process_runs_session_id
            ON process_runs(session_id, started_at_ms DESC);
        CREATE INDEX idx_process_runs_runtime_id
            ON process_runs(runtime_id, started_at_ms DESC);

        CREATE TABLE session_turns (
            turn_id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            run_id INTEGER NOT NULL,
            pid INTEGER NOT NULL,
            turn_index INTEGER NOT NULL,
            workload TEXT NOT NULL,
            source TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            completed_at_ms INTEGER NULL,
            finish_reason TEXT NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE CASCADE,
            FOREIGN KEY(run_id) REFERENCES process_runs(run_id) ON DELETE CASCADE
        );

        CREATE UNIQUE INDEX idx_session_turns_session_turn_index
            ON session_turns(session_id, turn_index);
        CREATE INDEX idx_session_turns_session_started_at
            ON session_turns(session_id, started_at_ms ASC);
        CREATE INDEX idx_session_turns_run_id
            ON session_turns(run_id, turn_index DESC);

        CREATE TABLE session_messages (
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

        CREATE UNIQUE INDEX idx_session_messages_turn_ordinal
            ON session_messages(turn_id, ordinal);
        CREATE INDEX idx_session_messages_session
            ON session_messages(session_id, message_id ASC);

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
            reservation_ram_bytes INTEGER NOT NULL DEFAULT 0,
            reservation_vram_bytes INTEGER NOT NULL DEFAULT 0,
            pinned INTEGER NOT NULL DEFAULT 0,
            transition_state TEXT NULL,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL,
            last_used_at_ms INTEGER NOT NULL
        );

        CREATE INDEX idx_runtime_instances_state
            ON runtime_instances(state);
        CREATE INDEX idx_runtime_instances_last_used
            ON runtime_instances(last_used_at_ms DESC);

        CREATE TABLE runtime_load_queue (
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

        CREATE INDEX idx_runtime_load_queue_state_requested
            ON runtime_load_queue(state, requested_at_ms ASC);
        CREATE INDEX idx_runtime_load_queue_runtime_key_state
            ON runtime_load_queue(runtime_key, state);

        CREATE TABLE accounting_events (
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

        CREATE INDEX idx_accounting_events_recorded_at
            ON accounting_events(recorded_at_ms DESC);
        CREATE INDEX idx_accounting_events_session_recorded
            ON accounting_events(session_id, recorded_at_ms DESC);
        CREATE INDEX idx_accounting_events_runtime_recorded
            ON accounting_events(runtime_id, recorded_at_ms DESC);
        CREATE INDEX idx_accounting_events_backend_recorded
            ON accounting_events(backend_id, recorded_at_ms DESC);
        CREATE INDEX idx_accounting_events_provider_model
            ON accounting_events(provider_id, model_id, recorded_at_ms DESC);

        CREATE TABLE audit_events (
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

        CREATE INDEX idx_audit_events_recorded_at
            ON audit_events(recorded_at_ms DESC);
        CREATE INDEX idx_audit_events_session_recorded
            ON audit_events(session_id, recorded_at_ms DESC);
        CREATE INDEX idx_audit_events_pid_recorded
            ON audit_events(pid, recorded_at_ms DESC);
        CREATE INDEX idx_audit_events_runtime_recorded
            ON audit_events(runtime_id, recorded_at_ms DESC);
        CREATE INDEX idx_audit_events_category_kind_recorded
            ON audit_events(category, kind, recorded_at_ms DESC);

        CREATE TABLE workflow_task_attempts (
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
            termination_reason TEXT NULL,
            PRIMARY KEY(orchestration_id, task_id, attempt),
            FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE SET NULL
        );

        CREATE INDEX idx_workflow_task_attempts_orch_task
            ON workflow_task_attempts(orchestration_id, task_id, attempt DESC);
        CREATE INDEX idx_workflow_task_attempts_status
            ON workflow_task_attempts(orchestration_id, status, updated_at_ms DESC);

        CREATE TABLE workflow_artifacts (
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

        CREATE INDEX idx_workflow_artifacts_orch_task
            ON workflow_artifacts(orchestration_id, producer_task_id, producer_attempt DESC);

        CREATE TABLE workflow_task_artifact_inputs (
            orchestration_id INTEGER NOT NULL,
            consumer_task_id TEXT NOT NULL,
            consumer_attempt INTEGER NOT NULL,
            artifact_id TEXT NOT NULL,
            producer_task_id TEXT NOT NULL,
            producer_attempt INTEGER NOT NULL,
            PRIMARY KEY(orchestration_id, consumer_task_id, consumer_attempt, artifact_id),
            FOREIGN KEY(artifact_id) REFERENCES workflow_artifacts(artifact_id) ON DELETE CASCADE
        );

        CREATE INDEX idx_workflow_task_artifact_inputs_consumer
            ON workflow_task_artifact_inputs(
                orchestration_id,
                consumer_task_id,
                consumer_attempt
            );

        CREATE TABLE scheduled_jobs (
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

        CREATE INDEX idx_scheduled_jobs_next_run
            ON scheduled_jobs(enabled, next_run_at_ms);
        CREATE INDEX idx_scheduled_jobs_active_orch
            ON scheduled_jobs(active_orchestration_id);

        CREATE TABLE scheduled_job_runs (
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

        CREATE INDEX idx_scheduled_job_runs_job_id
            ON scheduled_job_runs(job_id, run_id DESC);
        CREATE INDEX idx_scheduled_job_runs_orch
            ON scheduled_job_runs(orchestration_id);

        CREATE TABLE ipc_messages (
            message_id TEXT PRIMARY KEY,
            orchestration_id INTEGER NULL,
            sender_pid INTEGER NULL,
            sender_task_id TEXT NULL,
            sender_attempt INTEGER NULL,
            receiver_pid INTEGER NULL,
            receiver_task_id TEXT NULL,
            receiver_attempt INTEGER NULL,
            receiver_role TEXT NULL,
            message_type TEXT NOT NULL,
            channel TEXT NULL,
            payload_preview TEXT NOT NULL,
            payload_text TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at_ms INTEGER NOT NULL,
            delivered_at_ms INTEGER NULL,
            consumed_at_ms INTEGER NULL,
            failed_at_ms INTEGER NULL
        );

        CREATE INDEX idx_ipc_messages_orch_created
            ON ipc_messages(orchestration_id, created_at_ms DESC);
        CREATE INDEX idx_ipc_messages_sender_created
            ON ipc_messages(sender_pid, created_at_ms DESC);
        CREATE INDEX idx_ipc_messages_receiver_created
            ON ipc_messages(receiver_pid, created_at_ms DESC);
        CREATE INDEX idx_ipc_messages_status_created
            ON ipc_messages(status, created_at_ms DESC);
        CREATE INDEX idx_ipc_messages_task_created
            ON ipc_messages(orchestration_id, receiver_task_id, created_at_ms ASC);
        CREATE INDEX idx_ipc_messages_role_created
            ON ipc_messages(orchestration_id, receiver_role, created_at_ms ASC);
        CREATE INDEX idx_ipc_messages_channel_created
            ON ipc_messages(orchestration_id, channel, created_at_ms ASC);
        "#,
    )?;
    Ok(())
}

fn copy_legacy_rows(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    copy_kernel_meta(transaction)?;
    copy_kernel_boots(transaction)?;
    ensure_boot_seed(transaction)?;
    copy_sessions(transaction)?;
    copy_process_runs(transaction)?;
    ensure_process_runs_for_legacy_turns(transaction)?;
    copy_session_turns(transaction)?;
    copy_session_messages(transaction)?;
    copy_runtime_instances(transaction)?;
    copy_runtime_load_queue(transaction)?;
    copy_accounting_events(transaction)?;
    copy_audit_events(transaction)?;
    copy_workflow_task_attempts(transaction)?;
    copy_workflow_artifacts(transaction)?;
    copy_workflow_artifact_inputs(transaction)?;
    copy_scheduled_jobs(transaction)?;
    copy_scheduled_job_runs(transaction)?;
    copy_ipc_messages(transaction)?;
    Ok(())
}

fn copy_kernel_meta(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    let legacy = legacy_table_name("kernel_meta");
    if table_exists(transaction, &legacy)? {
        transaction.execute(
            &format!(
                "INSERT INTO kernel_meta (key, value, updated_at_ms) \
                 SELECT key, value, updated_at_ms FROM {legacy}"
            ),
            [],
        )?;
    }
    Ok(())
}

fn copy_kernel_boots(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    let legacy = legacy_table_name("kernel_boots");
    if table_exists(transaction, &legacy)? {
        transaction.execute(
            &format!(
                "INSERT INTO kernel_boots (boot_id, started_at_ms, kernel_version) \
                 SELECT boot_id, started_at_ms, kernel_version FROM {legacy}"
            ),
            [],
        )?;
    }
    Ok(())
}

fn ensure_boot_seed(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    let boot_count: i64 =
        transaction.query_row("SELECT COUNT(*) FROM kernel_boots", [], |row| row.get(0))?;
    if boot_count == 0 {
        transaction.execute(
            "INSERT INTO kernel_boots (boot_id, started_at_ms, kernel_version) VALUES (1, 0, 'rebaseline')",
            [],
        )?;
    }
    Ok(())
}

fn copy_sessions(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    let legacy = legacy_table_name("sessions");
    if !table_exists(transaction, &legacy)? {
        return Ok(());
    }
    let runtime_id_expr = if column_exists(transaction, &legacy, "runtime_id")? {
        "runtime_id"
    } else {
        "NULL AS runtime_id"
    };
    transaction.execute(
        &format!(
            "INSERT INTO sessions (session_id, title, status, runtime_id, active_pid, created_at_ms, updated_at_ms) \
             SELECT session_id, title, status, {runtime_id_expr}, active_pid, created_at_ms, updated_at_ms FROM {legacy}"
        ),
        [],
    )?;
    Ok(())
}

fn copy_process_runs(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    let legacy = legacy_table_name("process_runs");
    if !table_exists(transaction, &legacy)? {
        return Ok(());
    }
    let runtime_id_expr = if column_exists(transaction, &legacy, "runtime_id")? {
        "runtime_id"
    } else {
        "NULL AS runtime_id"
    };
    transaction.execute(
        &format!(
            "INSERT INTO process_runs (run_id, session_id, boot_id, pid, runtime_id, state, started_at_ms, ended_at_ms) \
             SELECT run_id, session_id, boot_id, pid, {runtime_id_expr}, state, started_at_ms, ended_at_ms FROM {legacy}"
        ),
        [],
    )?;
    Ok(())
}

fn ensure_process_runs_for_legacy_turns(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    let legacy = legacy_table_name("session_turns");
    if !table_exists(transaction, &legacy)? || column_exists(transaction, &legacy, "run_id")? {
        return Ok(());
    }

    let boot_id: i64 =
        transaction.query_row("SELECT boot_id FROM kernel_boots ORDER BY boot_id DESC LIMIT 1", [], |row| row.get(0))?;
    let runtime_id_expr = if table_exists(transaction, &legacy_table_name("sessions"))?
        && column_exists(transaction, &legacy_table_name("sessions"), "runtime_id")?
    {
        "s.runtime_id"
    } else {
        "NULL"
    };

    transaction.execute(
        &format!(
            r#"
            INSERT INTO process_runs (session_id, boot_id, pid, runtime_id, state, started_at_ms, ended_at_ms)
            SELECT
                lt.session_id,
                {boot_id},
                lt.pid,
                {runtime_id_expr},
                'recovered_legacy',
                MIN(lt.started_at_ms),
                MAX(COALESCE(lt.completed_at_ms, lt.updated_at_ms))
            FROM {legacy} lt
            LEFT JOIN {legacy_sessions} s ON s.session_id = lt.session_id
            LEFT JOIN process_runs pr ON pr.session_id = lt.session_id AND pr.pid = lt.pid
            WHERE pr.run_id IS NULL
            GROUP BY lt.session_id, lt.pid
            "#,
            legacy_sessions = legacy_table_name("sessions"),
        ),
        [],
    )?;

    Ok(())
}

fn copy_session_turns(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    let legacy = legacy_table_name("session_turns");
    if !table_exists(transaction, &legacy)? {
        return Ok(());
    }

    let run_id_expr = if column_exists(transaction, &legacy, "run_id")? {
        "COALESCE(run_id, (SELECT pr.run_id FROM process_runs pr WHERE pr.session_id = lt.session_id AND pr.pid = lt.pid ORDER BY pr.run_id DESC LIMIT 1))".to_string()
    } else {
        "(SELECT pr.run_id FROM process_runs pr WHERE pr.session_id = lt.session_id AND pr.pid = lt.pid ORDER BY pr.run_id DESC LIMIT 1)".to_string()
    };

    transaction.execute(
        &format!(
            "INSERT INTO session_turns (turn_id, session_id, run_id, pid, turn_index, workload, source, status, started_at_ms, updated_at_ms, completed_at_ms, finish_reason) \
             SELECT lt.turn_id, lt.session_id, {run_id_expr}, lt.pid, lt.turn_index, lt.workload, lt.source, lt.status, lt.started_at_ms, lt.updated_at_ms, lt.completed_at_ms, lt.finish_reason \
             FROM {legacy} lt"
        ),
        [],
    )?;
    Ok(())
}

fn copy_session_messages(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    let legacy = legacy_table_name("session_messages");
    if !table_exists(transaction, &legacy)? {
        return Ok(());
    }
    transaction.execute(
        &format!(
            "INSERT INTO session_messages (message_id, session_id, turn_id, pid, ordinal, role, kind, content, created_at_ms) \
             SELECT message_id, session_id, turn_id, pid, ordinal, role, CASE WHEN role = 'assistant' AND kind = 'chunk' THEN 'message' ELSE kind END, content, created_at_ms FROM {legacy}"
        ),
        [],
    )?;
    Ok(())
}

fn copy_runtime_instances(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    let legacy = legacy_table_name("runtime_instances");
    if !table_exists(transaction, &legacy)? {
        return Ok(());
    }
    let reservation_ram_expr = legacy_column_expr(transaction, &legacy, "reservation_ram_bytes", "0")?;
    let reservation_vram_expr = legacy_column_expr(transaction, &legacy, "reservation_vram_bytes", "0")?;
    let pinned_expr = legacy_column_expr(transaction, &legacy, "pinned", "0")?;
    let transition_expr = legacy_column_expr(transaction, &legacy, "transition_state", "NULL")?;
    transaction.execute(
        &format!(
            "INSERT INTO runtime_instances (runtime_id, runtime_key, state, target_kind, logical_model_id, display_path, runtime_reference, family, backend_id, backend_class, driver_source, driver_rationale, provider_id, remote_model_id, load_mode, reservation_ram_bytes, reservation_vram_bytes, pinned, transition_state, created_at_ms, updated_at_ms, last_used_at_ms) \
             SELECT runtime_id, runtime_key, state, target_kind, logical_model_id, display_path, runtime_reference, family, backend_id, backend_class, driver_source, driver_rationale, provider_id, remote_model_id, load_mode, {reservation_ram_expr}, {reservation_vram_expr}, {pinned_expr}, {transition_expr}, created_at_ms, updated_at_ms, last_used_at_ms FROM {legacy}"
        ),
        [],
    )?;
    Ok(())
}

fn copy_runtime_load_queue(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    copy_same_columns_if_table_exists(
        transaction,
        "runtime_load_queue",
        &[
            "queue_id",
            "runtime_key",
            "logical_model_id",
            "display_path",
            "backend_class",
            "state",
            "reservation_ram_bytes",
            "reservation_vram_bytes",
            "reason",
            "requested_at_ms",
            "updated_at_ms",
        ],
    )
}

fn copy_accounting_events(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    copy_same_columns_if_table_exists(
        transaction,
        "accounting_events",
        &[
            "event_id",
            "recorded_at_ms",
            "session_id",
            "pid",
            "runtime_id",
            "backend_id",
            "backend_class",
            "provider_id",
            "model_id",
            "request_kind",
            "status",
            "request_count",
            "stream",
            "input_tokens",
            "output_tokens",
            "estimated_cost_usd",
            "error_code",
            "error_message",
        ],
    )
}

fn copy_audit_events(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    copy_same_columns_if_table_exists(
        transaction,
        "audit_events",
        &[
            "audit_id",
            "recorded_at_ms",
            "category",
            "kind",
            "title",
            "detail",
            "session_id",
            "pid",
            "runtime_id",
        ],
    )
}

fn copy_workflow_task_attempts(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    let legacy = legacy_table_name("workflow_task_attempts");
    if !table_exists(transaction, &legacy)? {
        return Ok(());
    }
    let termination_expr = legacy_column_expr(transaction, &legacy, "termination_reason", "NULL")?;
    transaction.execute(
        &format!(
            "INSERT INTO workflow_task_attempts (orchestration_id, task_id, attempt, status, session_id, pid, error, output_preview, output_chars, truncated, started_at_ms, updated_at_ms, completed_at_ms, primary_artifact_id, termination_reason) \
             SELECT orchestration_id, task_id, attempt, status, session_id, pid, error, output_preview, output_chars, truncated, started_at_ms, updated_at_ms, completed_at_ms, primary_artifact_id, {termination_expr} FROM {legacy}"
        ),
        [],
    )?;
    Ok(())
}

fn copy_workflow_artifacts(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    copy_same_columns_if_table_exists(
        transaction,
        "workflow_artifacts",
        &[
            "artifact_id",
            "orchestration_id",
            "producer_task_id",
            "producer_attempt",
            "kind",
            "label",
            "mime_type",
            "content_text",
            "preview",
            "bytes",
            "created_at_ms",
        ],
    )
}

fn copy_workflow_artifact_inputs(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    copy_same_columns_if_table_exists(
        transaction,
        "workflow_task_artifact_inputs",
        &[
            "orchestration_id",
            "consumer_task_id",
            "consumer_attempt",
            "artifact_id",
            "producer_task_id",
            "producer_attempt",
        ],
    )
}

fn copy_scheduled_jobs(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    copy_same_columns_if_table_exists(
        transaction,
        "scheduled_jobs",
        &[
            "job_id",
            "name",
            "target_kind",
            "workflow_payload",
            "trigger_kind",
            "trigger_payload",
            "timeout_ms",
            "max_retries",
            "backoff_ms",
            "enabled",
            "state",
            "next_run_at_ms",
            "current_trigger_at_ms",
            "current_attempt",
            "active_run_id",
            "active_orchestration_id",
            "active_deadline_at_ms",
            "last_run_started_at_ms",
            "last_run_completed_at_ms",
            "last_run_status",
            "last_error",
            "consecutive_failures",
            "created_at_ms",
            "updated_at_ms",
        ],
    )
}

fn copy_scheduled_job_runs(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    copy_same_columns_if_table_exists(
        transaction,
        "scheduled_job_runs",
        &[
            "run_id",
            "job_id",
            "trigger_at_ms",
            "attempt",
            "status",
            "started_at_ms",
            "completed_at_ms",
            "orchestration_id",
            "deadline_at_ms",
            "error",
        ],
    )
}

fn copy_ipc_messages(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    let legacy = legacy_table_name("ipc_messages");
    if !table_exists(transaction, &legacy)? {
        return Ok(());
    }
    let receiver_role_expr = legacy_column_expr(transaction, &legacy, "receiver_role", "NULL")?;
    let failed_at_expr = legacy_column_expr(transaction, &legacy, "failed_at_ms", "NULL")?;
    transaction.execute(
        &format!(
            "INSERT INTO ipc_messages (message_id, orchestration_id, sender_pid, sender_task_id, sender_attempt, receiver_pid, receiver_task_id, receiver_attempt, receiver_role, message_type, channel, payload_preview, payload_text, status, created_at_ms, delivered_at_ms, consumed_at_ms, failed_at_ms) \
             SELECT message_id, orchestration_id, sender_pid, sender_task_id, sender_attempt, receiver_pid, receiver_task_id, receiver_attempt, {receiver_role_expr}, message_type, channel, payload_preview, payload_text, status, created_at_ms, delivered_at_ms, consumed_at_ms, {failed_at_expr} FROM {legacy}"
        ),
        [],
    )?;
    Ok(())
}

fn copy_same_columns_if_table_exists(
    transaction: &Transaction<'_>,
    table: &str,
    columns: &[&str],
) -> Result<(), StorageError> {
    let legacy = legacy_table_name(table);
    if !table_exists(transaction, &legacy)? {
        return Ok(());
    }
    let joined = columns.join(", ");
    transaction.execute(
        &format!("INSERT INTO {table} ({joined}) SELECT {joined} FROM {legacy}"),
        [],
    )?;
    Ok(())
}

fn drop_legacy_tables(transaction: &Transaction<'_>) -> Result<(), StorageError> {
    for table in LEGACY_TABLES {
        let legacy = legacy_table_name(table);
        if table_exists(transaction, &legacy)? {
            transaction.execute(&format!("DROP TABLE {legacy}"), [])?;
        }
    }
    Ok(())
}

fn table_exists(transaction: &Transaction<'_>, table: &str) -> Result<bool, rusqlite::Error> {
    transaction
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
}

fn column_exists(
    transaction: &Transaction<'_>,
    table: &str,
    column: &str,
) -> Result<bool, rusqlite::Error> {
    let mut statement = transaction.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row?.eq(column) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn legacy_column_expr(
    transaction: &Transaction<'_>,
    table: &str,
    column: &str,
    fallback_sql: &str,
) -> Result<String, StorageError> {
    if column_exists(transaction, table, column)? {
        Ok(column.to_string())
    } else {
        Ok(fallback_sql.to_string())
    }
}

fn legacy_table_name(table: &str) -> String {
    format!("__legacy_{table}")
}
