use std::collections::{BTreeMap, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::storage::{StorageError, StorageService, StoredSessionRecord};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionState {
    Idle,
    Running,
}

impl SessionState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
        }
    }

    fn parse(raw: &str) -> Self {
        match raw {
            "running" => Self::Running,
            _ => Self::Idle,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionRecord {
    pub(crate) session_id: String,
    pub(crate) title: String,
    pub(crate) state: SessionState,
    pub(crate) runtime_id: Option<String>,
    pub(crate) active_pid: Option<u64>,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
}

#[derive(Debug, Error)]
pub(crate) enum SessionRegistryError {
    #[error("{0}")]
    Storage(#[from] StorageError),

    #[error("session '{0}' not found")]
    SessionNotFound(String),
}

pub(crate) struct SessionRegistry {
    boot_id: i64,
    next_sequence: u64,
    pub(super) sessions: BTreeMap<String, SessionRecord>,
    pid_to_session: HashMap<u64, String>,
    pid_to_active_turn: HashMap<u64, i64>,
}

impl SessionRegistry {
    pub(crate) fn load(
        storage: &mut StorageService,
        boot_id: i64,
    ) -> Result<Self, SessionRegistryError> {
        storage.reset_active_sessions_for_boot()?;
        let stored_sessions = storage.load_sessions()?;
        let next_sequence = compute_next_sequence(boot_id, &stored_sessions);
        let mut sessions = BTreeMap::new();
        let mut pid_to_session = HashMap::new();

        for stored in stored_sessions {
            if let Some(pid) = stored.active_pid {
                pid_to_session.insert(pid, stored.session_id.clone());
            }

            sessions.insert(
                stored.session_id.clone(),
                SessionRecord {
                    session_id: stored.session_id,
                    title: stored.title,
                    state: SessionState::parse(&stored.status),
                    runtime_id: stored.runtime_id,
                    active_pid: stored.active_pid,
                    created_at_ms: stored.created_at_ms,
                    updated_at_ms: stored.updated_at_ms,
                },
            );
        }

        Ok(Self {
            boot_id,
            next_sequence,
            sessions,
            pid_to_session,
            pid_to_active_turn: HashMap::new(),
        })
    }

    pub(crate) fn open_session(
        &mut self,
        storage: &mut StorageService,
        prompt: &str,
        runtime_id: &str,
    ) -> Result<String, SessionRegistryError> {
        let session_id = self.allocate_session_id();
        let now = current_timestamp_ms();
        let record = SessionRecord {
            session_id: session_id.clone(),
            title: session_title_from_prompt(prompt),
            state: SessionState::Idle,
            runtime_id: Some(runtime_id.to_string()),
            active_pid: None,
            created_at_ms: now,
            updated_at_ms: now,
        };

        storage.insert_session(
            &record.session_id,
            &record.title,
            record.state.as_str(),
            record.runtime_id.as_deref(),
            record.active_pid,
            record.created_at_ms,
            record.updated_at_ms,
        )?;
        self.sessions.insert(record.session_id.clone(), record);

        Ok(session_id)
    }

    pub(crate) fn delete_session(
        &mut self,
        storage: &mut StorageService,
        session_id: &str,
    ) -> Result<(), SessionRegistryError> {
        storage.delete_session(session_id)?;
        self.sessions.remove(session_id);
        self.pid_to_session.retain(|_, bound| bound != session_id);
        self.pid_to_active_turn
            .retain(|pid, _| self.pid_to_session.contains_key(pid));
        Ok(())
    }

    pub(crate) fn bind_pid(
        &mut self,
        storage: &mut StorageService,
        session_id: &str,
        runtime_id: &str,
        pid: u64,
    ) -> Result<(), SessionRegistryError> {
        let record = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| SessionRegistryError::SessionNotFound(session_id.to_string()))?;
        let now = current_timestamp_ms();

        storage.bind_session_to_pid(session_id, runtime_id, self.boot_id, pid, now)?;
        record.state = SessionState::Running;
        record.runtime_id = Some(runtime_id.to_string());
        record.active_pid = Some(pid);
        record.updated_at_ms = now;
        self.pid_to_session.insert(pid, session_id.to_string());

        Ok(())
    }

    pub(crate) fn release_pid(
        &mut self,
        storage: &mut StorageService,
        pid: u64,
        run_state: &str,
    ) -> Result<Option<String>, SessionRegistryError> {
        let Some(session_id) = self.pid_to_session.remove(&pid) else {
            return Ok(None);
        };
        let Some(record) = self.sessions.get_mut(&session_id) else {
            return Ok(Some(session_id));
        };
        let now = current_timestamp_ms();
        storage.release_session_pid(&session_id, self.boot_id, pid, run_state, now)?;
        record.state = SessionState::Idle;
        record.active_pid = None;
        record.updated_at_ms = now;

        Ok(Some(session_id))
    }

    pub(crate) fn remember_active_turn(&mut self, pid: u64, turn_id: i64) {
        self.pid_to_active_turn.insert(pid, turn_id);
    }

    pub(crate) fn active_turn_id_for_pid(&self, pid: u64) -> Option<i64> {
        self.pid_to_active_turn.get(&pid).copied()
    }

    pub(crate) fn clear_active_turn(&mut self, pid: u64) -> Option<i64> {
        self.pid_to_active_turn.remove(&pid)
    }

    pub(crate) fn session_id_for_pid(&self, pid: u64) -> Option<&str> {
        self.pid_to_session.get(&pid).map(String::as_str)
    }

    pub(crate) fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub(crate) fn session_count_for_runtime(&self, runtime_id: &str) -> usize {
        self.sessions
            .values()
            .filter(|record| record.runtime_id.as_deref() == Some(runtime_id))
            .count()
    }

    fn allocate_session_id(&mut self) -> String {
        let session_id = format!("sess-{}-{:06}", self.boot_id, self.next_sequence);
        self.next_sequence += 1;
        session_id
    }
}

fn compute_next_sequence(boot_id: i64, sessions: &[StoredSessionRecord]) -> u64 {
    let prefix = format!("sess-{boot_id}-");
    sessions
        .iter()
        .filter_map(|session| {
            session
                .session_id
                .strip_prefix(&prefix)
                .and_then(|suffix| suffix.parse::<u64>().ok())
        })
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

fn session_title_from_prompt(prompt: &str) -> String {
    let single_line = prompt.lines().next().unwrap_or_default().trim();
    if single_line.is_empty() {
        return "Untitled session".to_string();
    }

    const MAX_CHARS: usize = 72;
    let mut title = String::new();
    for ch in single_line.chars().take(MAX_CHARS) {
        title.push(ch);
    }
    if single_line.chars().count() > MAX_CHARS {
        title.push_str("...");
    }
    title
}

fn current_timestamp_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "tests/registry.rs"]
mod tests;
