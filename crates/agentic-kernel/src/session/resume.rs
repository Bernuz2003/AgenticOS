use super::{SessionRecord, SessionRegistry};

impl SessionRegistry {
    pub(crate) fn runtime_id_for_pid(&self, pid: u64) -> Option<&str> {
        let session_id = self.session_id_for_pid(pid)?;
        self.runtime_id_for_session(session_id)
    }

    pub(crate) fn runtime_id_for_session(&self, session_id: &str) -> Option<&str> {
        self.sessions
            .get(session_id)
            .and_then(|record| record.runtime_id.as_deref())
    }

    pub(crate) fn session_id_for_pid_or_fallback(&self, pid: u64) -> String {
        self.session_id_for_pid(pid)
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("pid-{pid}"))
    }

    pub(crate) fn session(&self, session_id: &str) -> Option<&SessionRecord> {
        self.sessions.get(session_id)
    }

    pub(crate) fn active_pid_for_session(&self, session_id: &str) -> Option<u64> {
        self.session(session_id)
            .and_then(|record| record.active_pid)
    }
}
