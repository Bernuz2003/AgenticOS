use std::collections::BTreeMap;

use agentic_control_models::{PidStatusResponse, StatusResponse};
use agentic_protocol::OpCode;

use super::mappers::{
    map_orchestration_status_to_workspace_snapshot, map_orchestration_to_summary,
    map_pid_status_to_workspace_snapshot, map_process_to_session, merge_live_session_summary,
};
use super::transport::{self, KernelBridge, KernelBridgeResult};
use crate::kernel::history;
use crate::models::kernel::{AgentSessionSummary, LobbySnapshot, WorkspaceSnapshot};

impl KernelBridge {
    pub fn fetch_lobby_snapshot(&mut self) -> LobbySnapshot {
        let archived_sessions = history::load_lobby_sessions(&self.workspace_root).unwrap_or_default();
        let persisted_runtime_instances =
            history::load_runtime_instances(&self.workspace_root).unwrap_or_default();
        let persisted_runtime_load_queue =
            history::load_runtime_load_queue(&self.workspace_root).unwrap_or_default();
        let global_audit_events =
            history::load_global_audit_events(&self.workspace_root, 16).unwrap_or_default();

        match self.fetch_status() {
            Ok(status) => {
                let mut persisted_sessions = archived_sessions
                    .into_iter()
                    .map(|session| (session.session_id.clone(), session))
                    .collect::<BTreeMap<_, _>>();
                let active_sessions: Vec<AgentSessionSummary> = status
                    .processes
                    .active_processes
                    .into_iter()
                    .map(map_process_to_session)
                    .map(|live| {
                        merge_live_session_summary(
                            persisted_sessions.remove(&live.session_id),
                            live,
                        )
                    })
                    .collect();

                let mut sessions = active_sessions;
                sessions.extend(persisted_sessions.into_values());
                sessions.retain(|session| session.orchestration_id.is_none());

                LobbySnapshot {
                    connected: true,
                    selected_model_id: status.model.selected_model_id,
                    loaded_model_id: status.model.loaded_model_id,
                    loaded_target_kind: status.model.loaded_target_kind,
                    loaded_provider_id: status.model.loaded_provider_id,
                    loaded_remote_model_id: status.model.loaded_remote_model_id,
                    loaded_backend_id: status.model.loaded_backend,
                    loaded_backend_class: status.model.loaded_backend_class,
                    loaded_backend_capabilities: status.model.loaded_backend_capabilities,
                    global_accounting: status.global_accounting,
                    loaded_backend_telemetry: status.model.loaded_backend_telemetry,
                    loaded_remote_model: status.model.loaded_remote_model,
                    memory: Some(status.memory),
                    runtime_instances: status.model.runtime_instances,
                    managed_local_runtimes: status.model.managed_local_runtimes,
                    resource_governor: status.model.resource_governor,
                    runtime_load_queue: status.model.runtime_load_queue,
                    global_audit_events,
                    scheduled_jobs: status.jobs.scheduled_jobs,
                    orchestrations: status
                        .orchestrations
                        .active_orchestrations
                        .into_iter()
                        .map(map_orchestration_to_summary)
                        .collect(),
                    sessions,
                    error: None,
                }
            }
            Err(err) => LobbySnapshot {
                connected: false,
                selected_model_id: String::new(),
                loaded_model_id: String::new(),
                loaded_target_kind: None,
                loaded_provider_id: None,
                loaded_remote_model_id: None,
                loaded_backend_id: None,
                loaded_backend_class: None,
                loaded_backend_capabilities: None,
                global_accounting: history::load_global_accounting_summary(&self.workspace_root)
                    .unwrap_or_default(),
                loaded_backend_telemetry: None,
                loaded_remote_model: None,
                memory: None,
                runtime_instances: persisted_runtime_instances,
                managed_local_runtimes: Vec::new(),
                resource_governor: None,
                runtime_load_queue: persisted_runtime_load_queue,
                global_audit_events,
                scheduled_jobs: Vec::new(),
                orchestrations: Vec::new(),
                sessions: archived_sessions
                    .into_iter()
                    .filter(|session| session.orchestration_id.is_none())
                    .collect(),
                error: Some(err.to_string()),
            },
        }
    }

    pub fn find_live_pid_for_session(
        &mut self,
        session_id: &str,
    ) -> KernelBridgeResult<Option<u64>> {
        let status = self.fetch_status()?;
        Ok(status
            .processes
            .active_processes
            .into_iter()
            .find(|process| process.session_id == session_id)
            .map(|process| process.pid))
    }

    pub fn find_live_pids_for_session(
        &mut self,
        session_id: &str,
    ) -> KernelBridgeResult<Vec<u64>> {
        let status = self.fetch_status()?;
        Ok(status
            .processes
            .active_processes
            .into_iter()
            .filter(|process| process.session_id == session_id)
            .map(|process| process.pid)
            .collect())
    }

    pub fn fetch_workspace_snapshot(
        &mut self,
        pid: u64,
    ) -> KernelBridgeResult<WorkspaceSnapshot> {
        let payload = pid.to_string();
        let response = self.send_control_command(OpCode::Status, payload.as_bytes())?;

        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        let status = self.decode_response::<PidStatusResponse>(
            &response.payload,
            &[agentic_protocol::schema::PID_STATUS],
        )?;
        let session_id = status.session_id.clone();
        let (orchestration, orchestration_fetch_error) =
            if let Some(orch_id) = status.orchestration_id {
                match self.fetch_orchestration_status(orch_id) {
                    Ok(orchestration) => (
                        Some(map_orchestration_status_to_workspace_snapshot(
                            orchestration,
                            status.orchestration_task_id.clone(),
                        )),
                        None,
                    ),
                    Err(err) => (None, Some(err)),
                }
            } else {
                (None, None)
            };
        let mut snapshot = map_pid_status_to_workspace_snapshot(
            status,
            orchestration,
            orchestration_fetch_error.map(|err| err.to_string()),
        );
        if let Ok(audit_events) = history::load_session_audit_events(&self.workspace_root, &session_id, 64)
        {
            snapshot.audit_events = audit_events;
        }
        Ok(snapshot)
    }

    fn fetch_status(&mut self) -> KernelBridgeResult<StatusResponse> {
        let response = self.send_control_command(OpCode::Status, &[])?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::STATUS])
    }
}
