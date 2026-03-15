use super::{NewAuditEvent, StorageService};
use rusqlite::params;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn audit_events_survive_reopen_and_filter_by_session() {
    let dir = make_temp_dir("agenticos-audit-storage");
    let db_path = dir.join("agenticos.db");

    {
        let mut storage = StorageService::open(&db_path).expect("open storage");
        storage
            .insert_session("sess-1", "Session one", "idle", None, None, 1_000, 1_000)
            .expect("insert session 1");
        storage
            .insert_session("sess-2", "Session two", "idle", None, None, 1_000, 1_000)
            .expect("insert session 2");
        storage
            .record_audit_event(&NewAuditEvent {
                category: "process".to_string(),
                kind: "spawned".to_string(),
                title: "Process spawned".to_string(),
                detail: "pid=7".to_string(),
                session_id: Some("sess-1".to_string()),
                pid: Some(7),
                runtime_id: None,
            })
            .expect("record event 1");
        storage
            .record_audit_event(&NewAuditEvent {
                category: "tool".to_string(),
                kind: "completed".to_string(),
                title: "Tool completed".to_string(),
                detail: "pid=8".to_string(),
                session_id: Some("sess-2".to_string()),
                pid: Some(8),
                runtime_id: None,
            })
            .expect("record event 2");
        assert_eq!(storage.audit_event_count().expect("audit count"), 2);
    }

    let reopened = StorageService::open(&db_path).expect("reopen storage");
    let global = reopened.recent_audit_events(16).expect("load global audit");
    let session = reopened
        .recent_audit_events_for_session("sess-1", 16)
        .expect("load session audit");

    assert_eq!(global.len(), 2);
    assert_eq!(session.len(), 1);
    assert_eq!(session[0].session_id.as_deref(), Some("sess-1"));
    assert_eq!(session[0].kind, "spawned");

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn audit_events_order_by_timestamp_and_id_for_replay() {
    let dir = make_temp_dir("agenticos-audit-ordering");
    let db_path = dir.join("agenticos.db");

    let mut storage = StorageService::open(&db_path).expect("open storage");
    storage
        .insert_session("sess-1", "Session one", "idle", None, None, 1_000, 1_000)
        .expect("insert session 1");
    storage
        .insert_session("sess-2", "Session two", "idle", None, None, 1_000, 1_000)
        .expect("insert session 2");
    storage
        .connection
        .execute(
            r#"
                INSERT INTO audit_events (
                    recorded_at_ms,
                    category,
                    kind,
                    title,
                    detail,
                    session_id,
                    pid,
                    runtime_id
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
            params![
                1_000_i64,
                "process",
                "spawned",
                "Process spawned",
                "pid=4",
                "sess-1",
                4_u64,
                Option::<String>::None
            ],
        )
        .expect("insert first audit event");
    storage
        .connection
        .execute(
            r#"
                INSERT INTO audit_events (
                    recorded_at_ms,
                    category,
                    kind,
                    title,
                    detail,
                    session_id,
                    pid,
                    runtime_id
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
            params![
                1_000_i64,
                "tool",
                "completed",
                "Tool completed",
                "pid=4",
                "sess-1",
                4_u64,
                Option::<String>::None
            ],
        )
        .expect("insert second audit event");
    storage
        .connection
        .execute(
            r#"
                INSERT INTO audit_events (
                    recorded_at_ms,
                    category,
                    kind,
                    title,
                    detail,
                    session_id,
                    pid,
                    runtime_id
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
            params![
                900_i64,
                "runtime",
                "ready",
                "Runtime ready",
                "pid=8",
                "sess-2",
                8_u64,
                Option::<String>::None
            ],
        )
        .expect("insert third audit event");

    let global = storage.recent_audit_events(8).expect("load global audit");
    let session = storage
        .recent_audit_events_for_session("sess-1", 8)
        .expect("load session audit");

    assert_eq!(global.len(), 3);
    assert_eq!(global[0].kind, "completed");
    assert_eq!(global[1].kind, "spawned");
    assert_eq!(global[2].kind, "ready");

    assert_eq!(session.len(), 2);
    assert_eq!(session[0].kind, "completed");
    assert_eq!(session[1].kind, "spawned");
    assert!(session[0].recorded_at_ms >= session[1].recorded_at_ms);

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
