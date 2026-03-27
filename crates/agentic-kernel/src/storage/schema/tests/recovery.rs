use super::BootRecoveryReport;
use crate::storage::StorageService;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn boot_recovery_interrupts_open_runs_and_turns() {
    let dir = make_temp_dir("agenticos-boot-recovery");
    let db_path = dir.join("agenticos.db");

    let mut storage = StorageService::open(&db_path).expect("open storage");
    let boot = storage
        .record_kernel_boot("0.5.0-test")
        .expect("record boot");
    storage
        .insert_session(
            "sess-1",
            "Recovered session",
            "running",
            Some("rt-local"),
            Some(7),
            1_000,
            1_000,
        )
        .expect("insert session");
    storage
        .upsert_runtime_instance(&crate::runtimes::StoredRuntimeRecord {
            runtime_id: "rt-local".to_string(),
            runtime_key: "local-key".to_string(),
            state: "registered".to_string(),
            target_kind: "model".to_string(),
            logical_model_id: "qwen".to_string(),
            display_path: "/tmp/qwen.gguf".to_string(),
            runtime_reference: "/tmp/qwen.gguf".to_string(),
            family: "Qwen".to_string(),
            backend_id: "external-llamacpp".to_string(),
            backend_class: "resident_local".to_string(),
            driver_source: "test".to_string(),
            driver_rationale: "test".to_string(),
            provider_id: None,
            remote_model_id: None,
            load_mode: "resident_local_adapter".to_string(),
            reservation_ram_bytes: 1,
            reservation_vram_bytes: 1,
            pinned: false,
            transition_state: None,
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
            last_used_at_ms: 1_000,
        })
        .expect("insert runtime");
    storage
        .bind_session_to_pid("sess-1", "rt-local", boot.boot_id, 7, 2_000)
        .expect("bind session");
    let _ = storage
        .start_session_turn("sess-1", 7, "general", "exec", "hello", "prompt")
        .expect("start turn");

    let report = storage.run_boot_recovery().expect("run boot recovery");

    assert_eq!(
        report,
        BootRecoveryReport {
            recovered_at_ms: report.recovered_at_ms,
            stale_active_sessions_reset: 1,
            interrupted_process_runs: 1,
            interrupted_turns: 1,
            interrupted_scheduler_job_runs: 0,
            logical_resume_sessions: 0,
            strong_restore_candidate_sessions: 1,
            pending_runtime_queue_entries: 0,
            persisted_sessions: 1,
            known_runtimes: 1,
        }
    );

    let session: (Option<u64>, String) = storage
        .connection
        .query_row(
            "SELECT active_pid, status FROM sessions WHERE session_id = 'sess-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load session");
    assert_eq!(session.0, None);
    assert_eq!(session.1, "idle");

    let run: (String, Option<i64>) = storage
        .connection
        .query_row(
            "SELECT state, ended_at_ms FROM process_runs WHERE session_id = 'sess-1' LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load process run");
    assert_eq!(run.0, "interrupted");
    assert!(run.1.is_some());

    let turn: (String, String) = storage
        .connection
        .query_row(
            "SELECT status, finish_reason FROM session_turns WHERE session_id = 'sess-1' LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("load turn");
    assert_eq!(turn.0, "interrupted");
    assert_eq!(turn.1, "kernel_restarted");

    let marker: String = storage
        .connection
        .query_row(
            r#"
                SELECT content
                FROM session_messages
                WHERE session_id = 'sess-1' AND role = 'system'
                ORDER BY message_id DESC
                LIMIT 1
                "#,
            [],
            |row| row.get(0),
        )
        .expect("load marker");
    assert!(marker.contains("strong-restore candidate"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn boot_recovery_reports_pending_runtime_queue_entries() {
    let dir = make_temp_dir("agenticos-boot-recovery-queue");
    let db_path = dir.join("agenticos.db");

    let mut storage = StorageService::open(&db_path).expect("open storage");
    storage
        .insert_runtime_load_queue_entry(
            "rt-1",
            "qwen",
            "/tmp/qwen.gguf",
            "resident_local",
            "pending",
            10,
            20,
            "waiting for VRAM",
        )
        .expect("insert pending entry");
    storage
        .insert_runtime_load_queue_entry(
            "rt-2",
            "gpt-4.1-mini",
            "openai://gpt-4.1-mini",
            "remote_stateless",
            "admitted",
            0,
            0,
            "already admitted",
        )
        .expect("insert admitted entry");

    let report = storage.run_boot_recovery().expect("run boot recovery");
    assert_eq!(report.pending_runtime_queue_entries, 1);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn boot_recovery_marks_remote_stateless_sessions_as_logical_resume_only() {
    let dir = make_temp_dir("agenticos-boot-recovery-remote");
    let db_path = dir.join("agenticos.db");

    let mut storage = StorageService::open(&db_path).expect("open storage");
    let boot = storage
        .record_kernel_boot("0.5.0-test")
        .expect("record boot");
    storage
        .insert_session(
            "sess-remote",
            "Recovered remote session",
            "running",
            Some("rt-remote"),
            Some(11),
            1_000,
            1_000,
        )
        .expect("insert session");
    storage
        .upsert_runtime_instance(&crate::runtimes::StoredRuntimeRecord {
            runtime_id: "rt-remote".to_string(),
            runtime_key: "remote-key".to_string(),
            state: "registered".to_string(),
            target_kind: "provider_model".to_string(),
            logical_model_id: "gpt-4.1-mini".to_string(),
            display_path: "openai://gpt-4.1-mini".to_string(),
            runtime_reference: "openai://gpt-4.1-mini".to_string(),
            family: "Chat".to_string(),
            backend_id: "openai-compatible".to_string(),
            backend_class: "remote_stateless".to_string(),
            driver_source: "test".to_string(),
            driver_rationale: "test".to_string(),
            provider_id: Some("openai".to_string()),
            remote_model_id: Some("gpt-4.1-mini".to_string()),
            load_mode: "remote_stateless_adapter".to_string(),
            reservation_ram_bytes: 0,
            reservation_vram_bytes: 0,
            pinned: false,
            transition_state: None,
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
            last_used_at_ms: 1_000,
        })
        .expect("insert runtime");
    storage
        .bind_session_to_pid("sess-remote", "rt-remote", boot.boot_id, 11, 2_000)
        .expect("bind session");
    let _ = storage
        .start_session_turn(
            "sess-remote",
            11,
            "general",
            "exec",
            "hello remote",
            "prompt",
        )
        .expect("start turn");

    let report = storage.run_boot_recovery().expect("run boot recovery");

    assert_eq!(report.interrupted_turns, 1);
    assert_eq!(report.logical_resume_sessions, 1);
    assert_eq!(report.strong_restore_candidate_sessions, 0);

    let marker: String = storage
        .connection
        .query_row(
            r#"
                SELECT content
                FROM session_messages
                WHERE session_id = 'sess-remote' AND role = 'system'
                ORDER BY message_id DESC
                LIMIT 1
                "#,
            [],
            |row| row.get(0),
        )
        .expect("load marker");
    assert!(marker.contains("cannot restore the live process"));
    assert!(marker.contains("persisted context only"));

    let _ = fs::remove_dir_all(dir);
}

fn make_temp_dir(prefix: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{timestamp}"));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
