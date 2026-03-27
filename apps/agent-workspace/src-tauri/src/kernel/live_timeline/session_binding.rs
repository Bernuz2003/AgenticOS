use super::store::{TimelineSessionState, TimelineStore};

impl TimelineStore {
    pub fn rebind_session_pid(&mut self, session_id: &str, pid: u64, workload: String) {
        let Some((existing_pid, existing_session)) = self
            .sessions
            .iter()
            .find(|(_, session)| session.session_id == session_id)
            .map(|(existing_pid, session)| (*existing_pid, session.clone()))
        else {
            return;
        };

        self.sessions.remove(&existing_pid);
        self.sessions.insert(
            pid,
            TimelineSessionState {
                session_id: existing_session.session_id,
                pid,
                workload,
                turns: existing_session.turns,
                error: existing_session.error,
                system_events: existing_session.system_events,
            },
        );
    }

    pub fn has_pid(&self, pid: u64) -> bool {
        self.sessions.contains_key(&pid)
    }

    pub fn has_session_id(&self, session_id: &str) -> bool {
        self.sessions
            .values()
            .any(|session| session.session_id == session_id)
    }
}
