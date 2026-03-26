use std::path::Path;
use std::sync::{Arc, Mutex};

use super::client::KernelBridge;
use super::live_cache::{self, TimelineStore};
use super::persisted_truth;
use crate::models::kernel::{TimelineSnapshot, WorkspaceSnapshot};

pub fn compose_workspace_snapshot_for_session(
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    session_id: &str,
    pid: Option<u64>,
) -> Result<WorkspaceSnapshot, String> {
    if let Some(pid) = pid {
        if let Some(snapshot) = try_fetch_live_snapshot_by_pid_for_session(bridge, session_id, pid)?
        {
            ensure_live_timeline_from_snapshot(workspace_root, timeline_store, snapshot.clone())?;
            return Ok(snapshot);
        }
        if let Some(snapshot) =
            persisted_truth::load_workspace_snapshot(workspace_root, session_id, Some(pid))?
        {
            return Ok(snapshot);
        }
    }

    if let Some(snapshot) = try_fetch_live_snapshot_for_session(bridge, session_id)? {
        ensure_live_timeline_from_snapshot(workspace_root, timeline_store, snapshot.clone())?;
        return Ok(snapshot);
    }

    let Some(persisted) =
        persisted_truth::load_workspace_snapshot(workspace_root, session_id, None)?
    else {
        return Err(format!(
            "No persisted workspace snapshot found for session {}",
            session_id
        ));
    };

    if let Some(active_pid) = persisted.active_pid {
        if let Some(snapshot) =
            try_fetch_live_snapshot_by_pid_for_session(bridge, session_id, active_pid)?
        {
            ensure_live_timeline_from_snapshot(workspace_root, timeline_store, snapshot.clone())?;
            return Ok(snapshot);
        }
    }

    Ok(persisted)
}

pub fn compose_workspace_snapshot_for_pid(
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    pid: u64,
) -> Result<WorkspaceSnapshot, String> {
    let Some(snapshot) = try_fetch_live_snapshot_by_pid(bridge, pid)? else {
        return Err(format!("No live workspace snapshot found for PID {}", pid));
    };
    ensure_live_timeline_from_snapshot(workspace_root, timeline_store, snapshot.clone())?;
    Ok(snapshot)
}

pub fn compose_timeline_snapshot_for_session(
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    session_id: &str,
    pid: Option<u64>,
) -> Result<TimelineSnapshot, String> {
    if let Some(timeline) = snapshot_live_timeline_for_session(timeline_store, session_id)? {
        return Ok(timeline);
    }

    let live_snapshot = if let Some(pid) = pid {
        try_fetch_live_snapshot_by_pid_for_session(bridge, session_id, pid)?
    } else {
        try_fetch_live_snapshot_for_session(bridge, session_id)?
    };

    if let Some(snapshot) = live_snapshot {
        ensure_live_timeline_from_snapshot(workspace_root, timeline_store, snapshot.clone())?;
        if let Some(timeline) = snapshot_live_timeline_for_session(timeline_store, session_id)? {
            return Ok(timeline);
        }
        return Ok(live_cache::synthesize_fallback_timeline(snapshot));
    }

    let persisted_workspace =
        persisted_truth::load_workspace_snapshot(workspace_root, session_id, pid)?;
    let resolved_pid = pid.or_else(|| {
        persisted_workspace
            .as_ref()
            .and_then(|snapshot| snapshot.active_pid.or(snapshot.last_pid))
    });

    if let Some(timeline) =
        persisted_truth::load_timeline_snapshot(workspace_root, session_id, resolved_pid)?
    {
        return Ok(timeline);
    }

    Err(format!(
        "No persisted timeline found for session {}",
        session_id
    ))
}

pub fn compose_timeline_snapshot_for_pid(
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    pid: u64,
) -> Result<Option<TimelineSnapshot>, String> {
    if let Some(timeline) = snapshot_live_timeline_for_pid(timeline_store, pid)? {
        return Ok(Some(timeline));
    }

    let Some(snapshot) = try_fetch_live_snapshot_by_pid(bridge, pid)? else {
        return Ok(None);
    };
    ensure_live_timeline_from_snapshot(workspace_root, timeline_store, snapshot.clone())?;

    if let Some(timeline) = snapshot_live_timeline_for_pid(timeline_store, pid)? {
        return Ok(Some(timeline));
    }

    Ok(Some(live_cache::synthesize_fallback_timeline(snapshot)))
}

pub fn ensure_live_timeline_for_pid(
    workspace_root: &Path,
    bridge: &Arc<Mutex<KernelBridge>>,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    pid: u64,
) -> Result<String, String> {
    let snapshot = compose_workspace_snapshot_for_pid(workspace_root, bridge, timeline_store, pid)?;
    Ok(snapshot.session_id)
}

pub fn register_live_user_input(
    workspace_root: &Path,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    session_id: &str,
    pid: u64,
    workload_hint: Option<String>,
    prompt: &str,
) -> Result<(), String> {
    ensure_live_timeline_for_session_pid(
        workspace_root,
        timeline_store,
        session_id,
        pid,
        workload_hint,
    )?;
    let mut store = timeline_store
        .lock()
        .map_err(|_| "Timeline store lock poisoned".to_string())?;
    store.append_user_turn(pid, prompt.to_string());
    Ok(())
}

pub fn ensure_live_timeline_from_snapshot(
    workspace_root: &Path,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    snapshot: WorkspaceSnapshot,
) -> Result<(), String> {
    ensure_live_timeline_for_session_pid(
        workspace_root,
        timeline_store,
        &snapshot.session_id,
        snapshot.pid,
        Some(snapshot.workload),
    )
}

fn ensure_live_timeline_for_session_pid(
    workspace_root: &Path,
    timeline_store: &Arc<Mutex<TimelineStore>>,
    session_id: &str,
    pid: u64,
    workload_hint: Option<String>,
) -> Result<(), String> {
    let workload = workload_hint
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            persisted_truth::load_workspace_snapshot(workspace_root, session_id, Some(pid))
                .ok()
                .flatten()
                .map(|snapshot| snapshot.workload)
        })
        .unwrap_or_else(|| "general".to_string());

    {
        let mut store = timeline_store
            .lock()
            .map_err(|_| "Timeline store lock poisoned".to_string())?;
        if store.has_pid(pid) {
            return Ok(());
        }
        if store.has_session_id(session_id) {
            store.rebind_session_pid(session_id, pid, workload);
            return Ok(());
        }
    }

    let seeded = persisted_truth::load_timeline_seed(workspace_root, session_id, Some(pid))?;

    let mut store = timeline_store
        .lock()
        .map_err(|_| "Timeline store lock poisoned".to_string())?;
    if let Some(mut seeded) = seeded {
        seeded.pid = pid;
        if !workload.trim().is_empty() {
            seeded.workload = workload;
        }
        store.insert_seeded_session(seeded);
    } else {
        store.insert_empty_session(pid, session_id.to_string(), workload);
    }
    Ok(())
}

fn snapshot_live_timeline_for_session(
    timeline_store: &Arc<Mutex<TimelineStore>>,
    session_id: &str,
) -> Result<Option<TimelineSnapshot>, String> {
    let store = timeline_store
        .lock()
        .map_err(|_| "Timeline store lock poisoned".to_string())?;
    Ok(store.snapshot_for_session_id(session_id))
}

fn snapshot_live_timeline_for_pid(
    timeline_store: &Arc<Mutex<TimelineStore>>,
    pid: u64,
) -> Result<Option<TimelineSnapshot>, String> {
    let store = timeline_store
        .lock()
        .map_err(|_| "Timeline store lock poisoned".to_string())?;
    Ok(store.snapshot(pid))
}

fn try_fetch_live_snapshot_for_session(
    bridge: &Arc<Mutex<KernelBridge>>,
    session_id: &str,
) -> Result<Option<WorkspaceSnapshot>, String> {
    let mut bridge = bridge
        .lock()
        .map_err(|_| "Bridge state lock poisoned".to_string())?;
    let live_pid = match bridge.find_live_pid_for_session(session_id) {
        Ok(pid) => pid,
        Err(_) => return Ok(None),
    };
    let Some(live_pid) = live_pid else {
        return Ok(None);
    };

    let Ok(snapshot) = bridge.fetch_workspace_snapshot(live_pid) else {
        return Ok(None);
    };
    if snapshot.session_id != session_id {
        return Ok(None);
    }
    Ok(Some(snapshot))
}

fn try_fetch_live_snapshot_by_pid(
    bridge: &Arc<Mutex<KernelBridge>>,
    pid: u64,
) -> Result<Option<WorkspaceSnapshot>, String> {
    let mut bridge = bridge
        .lock()
        .map_err(|_| "Bridge state lock poisoned".to_string())?;
    let Ok(snapshot) = bridge.fetch_workspace_snapshot(pid) else {
        return Ok(None);
    };
    Ok(Some(snapshot))
}

fn try_fetch_live_snapshot_by_pid_for_session(
    bridge: &Arc<Mutex<KernelBridge>>,
    session_id: &str,
    pid: u64,
) -> Result<Option<WorkspaceSnapshot>, String> {
    let Some(snapshot) = try_fetch_live_snapshot_by_pid(bridge, pid)? else {
        return Ok(None);
    };
    if snapshot.session_id != session_id {
        return Err(format!(
            "PID {} is associated with session {}, not {}",
            pid, snapshot.session_id, session_id
        ));
    }
    Ok(Some(snapshot))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::{params, Connection};

    use super::{
        compose_timeline_snapshot_for_session, compose_workspace_snapshot_for_session,
        register_live_user_input,
    };
    use crate::kernel::client::KernelBridge;
    use crate::kernel::live_cache::TimelineStore;

    #[test]
    fn workspace_composer_falls_back_to_persisted_snapshot_when_live_bridge_is_unavailable() {
        let root = make_temp_root("agenticos-composer-workspace");
        seed_persisted_session(&root, "sess-compose", 44, "completed").expect("seed session");
        let bridge = Arc::new(Mutex::new(KernelBridge::new(
            "127.0.0.1:9".to_string(),
            root.clone(),
        )));
        let timeline_store = Arc::new(Mutex::new(TimelineStore::default()));

        let snapshot = compose_workspace_snapshot_for_session(
            &root,
            &bridge,
            &timeline_store,
            "sess-compose",
            None,
        )
        .expect("compose workspace snapshot");

        assert_eq!(snapshot.session_id, "sess-compose");
        assert_eq!(snapshot.pid, 44);
        assert_eq!(snapshot.state, "Finished");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn timeline_composer_falls_back_to_persisted_history_when_live_bridge_is_unavailable() {
        let root = make_temp_root("agenticos-composer-timeline");
        seed_persisted_session(&root, "sess-compose", 44, "completed").expect("seed session");
        let bridge = Arc::new(Mutex::new(KernelBridge::new(
            "127.0.0.1:9".to_string(),
            root.clone(),
        )));
        let timeline_store = Arc::new(Mutex::new(TimelineStore::default()));

        let timeline = compose_timeline_snapshot_for_session(
            &root,
            &bridge,
            &timeline_store,
            "sess-compose",
            None,
        )
        .expect("compose timeline snapshot");

        assert_eq!(timeline.session_id, "sess-compose");
        assert_eq!(timeline.pid, 44);
        assert_eq!(timeline.source, "sqlite_history");
        assert_eq!(timeline.items.len(), 2);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn register_live_user_input_seeds_persisted_history_before_appending_new_turn() {
        let root = make_temp_root("agenticos-composer-resume-seed");
        seed_persisted_session(&root, "sess-compose", 44, "completed").expect("seed session");
        let timeline_store = Arc::new(Mutex::new(TimelineStore::default()));

        register_live_user_input(
            &root,
            &timeline_store,
            "sess-compose",
            84,
            Some("general".to_string()),
            "new input",
        )
        .expect("register live user input");

        let timeline = timeline_store
            .lock()
            .expect("timeline store")
            .snapshot(84)
            .expect("seeded live timeline");
        assert_eq!(timeline.session_id, "sess-compose");
        assert_eq!(timeline.items.len(), 4);
        assert_eq!(timeline.items[0].text, "hello composer");
        assert_eq!(timeline.items[1].text, "persisted answer");
        assert_eq!(timeline.items[2].text, "new input");
        assert!(timeline.running);

        let _ = fs::remove_dir_all(root);
    }

    fn seed_persisted_session(
        root: &PathBuf,
        session_id: &str,
        pid: u64,
        finish_reason: &str,
    ) -> Result<(), String> {
        let db_path = root.join("workspace").join("agenticos.db");
        fs::create_dir_all(db_path.parent().expect("db parent")).map_err(|err| err.to_string())?;
        let connection = Connection::open(&db_path).map_err(|err| err.to_string())?;
        connection
            .execute_batch(
                r#"
                CREATE TABLE sessions (
                    session_id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    status TEXT NOT NULL,
                    runtime_id TEXT NULL,
                    active_pid INTEGER NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                CREATE TABLE process_runs (
                    run_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    boot_id INTEGER NOT NULL,
                    pid INTEGER NOT NULL,
                    state TEXT NOT NULL,
                    started_at_ms INTEGER NOT NULL,
                    ended_at_ms INTEGER NULL
                );
                CREATE TABLE session_turns (
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
                    finish_reason TEXT NULL
                );
                CREATE TABLE session_messages (
                    message_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    turn_id INTEGER NOT NULL,
                    pid INTEGER NOT NULL,
                    ordinal INTEGER NOT NULL,
                    role TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL
                );
                "#,
            )
            .map_err(|err| err.to_string())?;
        connection
            .execute(
                "INSERT INTO sessions VALUES (?1, ?2, 'idle', NULL, NULL, 1, 10)",
                params![session_id, "Persisted session"],
            )
            .map_err(|err| err.to_string())?;
        connection
            .execute(
                "INSERT INTO process_runs (session_id, boot_id, pid, state, started_at_ms, ended_at_ms) VALUES (?1, 1, ?2, 'completed', 1, 2)",
                params![session_id, pid],
            )
            .map_err(|err| err.to_string())?;
        connection
            .execute(
                "INSERT INTO session_turns VALUES (1, ?1, ?2, 1, 'general', 'legacy', 'completed', 1, 1, 2, ?3)",
                params![session_id, pid, finish_reason],
            )
            .map_err(|err| err.to_string())?;
        connection
            .execute(
                "INSERT INTO session_messages VALUES (1, ?1, 1, ?2, 1, 'user', 'prompt', 'hello composer', 1)",
                params![session_id, pid],
            )
            .map_err(|err| err.to_string())?;
        connection
            .execute(
                "INSERT INTO session_messages VALUES (2, ?1, 1, ?2, 2, 'assistant', 'chunk', 'persisted answer', 2)",
                params![session_id, pid],
            )
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    fn make_temp_root(prefix: &str) -> PathBuf {
        let mut root = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("epoch")
            .as_nanos();
        root.push(format!("{prefix}-{nonce}"));
        root
    }
}
