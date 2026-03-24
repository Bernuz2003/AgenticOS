use std::collections::BTreeMap;
use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use agentic_control_models::{
    ArtifactListRequest, ArtifactListResponse, ControlMessage, LoadModelResult,
    ModelCatalogSnapshot, OrchStatusResponse, OrchSummaryResponse, OrchestrateResult,
    OrchestrationControlResult, OrchestrationListResponse, OrchestrationStatusRequest,
    PidStatusResponse, ResumeSessionResult, RetryTaskResult, ScheduleJobResult,
    ScheduledJobControlResult, ScheduledJobListResponse, SelectModelResult, SendInputResult,
    StatusResponse, TurnControlResult,
};
use agentic_protocol::OpCode;

use super::auth::kernel_token_path;
use super::error::{KernelBridgeError, KernelBridgeResult};
use super::persisted_truth;
use super::protocol;
use crate::models::kernel::{
    AgentSessionSummary, LobbyOrchestrationSummary, LobbySnapshot, WorkspaceContextSnapshot,
    WorkspaceHumanInputRequest, WorkspaceOrchestrationSnapshot, WorkspaceOrchestrationTask,
    WorkspaceSnapshot,
};

#[derive(Debug)]
pub struct KernelBridge {
    addr: String,
    workspace_root: PathBuf,
    stream: Option<TcpStream>,
}

impl KernelBridge {
    pub fn new(addr: String, workspace_root: PathBuf) -> Self {
        Self {
            addr,
            workspace_root,
            stream: None,
        }
    }

    pub fn fetch_lobby_snapshot(&mut self) -> LobbySnapshot {
        let archived_sessions =
            persisted_truth::load_lobby_sessions(&self.workspace_root).unwrap_or_default();
        let persisted_runtime_instances =
            persisted_truth::load_runtime_instances(&self.workspace_root).unwrap_or_default();
        let persisted_runtime_load_queue =
            persisted_truth::load_runtime_load_queue(&self.workspace_root).unwrap_or_default();
        let global_audit_events =
            persisted_truth::load_global_audit_events(&self.workspace_root, 16).unwrap_or_default();
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
                global_accounting: persisted_truth::load_global_accounting_summary(
                    &self.workspace_root,
                )
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

    pub fn find_live_pids_for_session(&mut self, session_id: &str) -> KernelBridgeResult<Vec<u64>> {
        let status = self.fetch_status()?;
        Ok(status
            .processes
            .active_processes
            .into_iter()
            .filter(|process| process.session_id == session_id)
            .map(|process| process.pid)
            .collect())
    }

    pub fn fetch_workspace_snapshot(&mut self, pid: u64) -> KernelBridgeResult<WorkspaceSnapshot> {
        let payload = pid.to_string();
        let response = self.send_control_command(OpCode::Status, payload.as_bytes())?;

        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
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
        if let Ok(audit_events) =
            persisted_truth::load_session_audit_events(&self.workspace_root, &session_id, 64)
        {
            snapshot.audit_events = audit_events;
        }
        Ok(snapshot)
    }

    fn fetch_status(&mut self) -> KernelBridgeResult<StatusResponse> {
        let response = self.send_control_command(OpCode::Status, &[])?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::STATUS])
    }

    pub fn fetch_orchestration_status(
        &mut self,
        orch_id: u64,
    ) -> KernelBridgeResult<OrchStatusResponse> {
        let payload = serde_json::to_vec(&OrchestrationStatusRequest {
            orchestration_id: orch_id,
        })?;
        let response = self.send_control_command(OpCode::OrchestrationStatus, &payload)?;

        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        let orchestration = self.decode_response::<OrchStatusResponse>(
            &response.payload,
            &[agentic_protocol::schema::ORCHESTRATION_STATUS],
        )?;
        Ok(orchestration)
    }

    pub fn list_orchestrations(&mut self) -> KernelBridgeResult<OrchestrationListResponse> {
        let response = self.send_control_command(OpCode::ListOrchestrations, &[])?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::LIST_ORCHESTRATIONS],
        )
    }

    pub fn list_scheduled_jobs(&mut self) -> KernelBridgeResult<ScheduledJobListResponse> {
        let response = self.send_control_command(OpCode::ListJobs, &[])?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::LIST_JOBS])
    }

    pub fn list_artifacts(
        &mut self,
        orchestration_id: u64,
        task: Option<&str>,
    ) -> KernelBridgeResult<ArtifactListResponse> {
        let payload = serde_json::to_vec(&ArtifactListRequest {
            orchestration_id,
            task: task.map(ToOwned::to_owned),
        })?;
        let response = self.send_control_command(OpCode::ListArtifacts, &payload)?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::LIST_ARTIFACTS],
        )
    }

    pub fn orchestrate(&mut self, payload: &str) -> KernelBridgeResult<OrchestrateResult> {
        let response = self.send_control_command(OpCode::Orchestrate, payload.as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::ORCHESTRATE])
    }

    pub fn schedule_job(&mut self, payload: &str) -> KernelBridgeResult<ScheduleJobResult> {
        let response = self.send_control_command(OpCode::ScheduleJob, payload.as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::SCHEDULE_JOB])
    }

    pub fn retry_task(
        &mut self,
        orch_id: u64,
        task_id: &str,
    ) -> KernelBridgeResult<RetryTaskResult> {
        let payload = serde_json::json!({
            "orchestration_id": orch_id,
            "task_id": task_id,
        });
        let response =
            self.send_control_command(OpCode::RetryTask, payload.to_string().as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::RETRY_TASK])
    }

    pub fn stop_orchestration(
        &mut self,
        orch_id: u64,
    ) -> KernelBridgeResult<OrchestrationControlResult> {
        let payload = serde_json::json!({ "orchestration_id": orch_id });
        let response =
            self.send_control_command(OpCode::StopOrchestration, payload.to_string().as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::STOP_ORCHESTRATION],
        )
    }

    pub fn delete_orchestration(
        &mut self,
        orch_id: u64,
    ) -> KernelBridgeResult<OrchestrationControlResult> {
        let payload = serde_json::json!({ "orchestration_id": orch_id });
        let response =
            self.send_control_command(OpCode::DeleteOrchestration, payload.to_string().as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::DELETE_ORCHESTRATION],
        )
    }

    pub fn set_job_enabled(
        &mut self,
        job_id: u64,
        enabled: bool,
    ) -> KernelBridgeResult<ScheduledJobControlResult> {
        let payload = serde_json::json!({ "job_id": job_id, "enabled": enabled });
        let response =
            self.send_control_command(OpCode::SetJobEnabled, payload.to_string().as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::SET_JOB_ENABLED],
        )
    }

    pub fn delete_job(&mut self, job_id: u64) -> KernelBridgeResult<ScheduledJobControlResult> {
        let payload = serde_json::json!({ "job_id": job_id });
        let response =
            self.send_control_command(OpCode::DeleteJob, payload.to_string().as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::DELETE_JOB])
    }

    pub fn ping(&mut self) -> KernelBridgeResult<String> {
        let response = self.send_control_command(OpCode::Ping, &[])?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        let message = self.decode_response::<ControlMessage>(
            &response.payload,
            &[agentic_protocol::schema::PING],
        )?;
        Ok(message.message)
    }

    pub fn list_models(&mut self) -> KernelBridgeResult<ModelCatalogSnapshot> {
        let response = self.send_control_command(OpCode::ListModels, &[])?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::LIST_MODELS])
    }

    pub fn select_model(&mut self, model_id: &str) -> KernelBridgeResult<SelectModelResult> {
        let response = self.send_control_command(OpCode::SelectModel, model_id.as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::SELECT_MODEL])
    }

    pub fn load_model(&mut self, selector: &str) -> KernelBridgeResult<LoadModelResult> {
        let response = self.send_control_command(OpCode::Load, selector.as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::LOAD])
    }

    pub fn send_input(&mut self, pid: u64, prompt: &str) -> KernelBridgeResult<SendInputResult> {
        let payload = serde_json::to_vec(&serde_json::json!({
            "pid": pid,
            "prompt": prompt,
        }))?;
        let response = self.send_control_command(OpCode::SendInput, &payload)?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::SEND_INPUT])
    }

    pub fn resume_session(&mut self, session_id: &str) -> KernelBridgeResult<ResumeSessionResult> {
        let payload = serde_json::to_vec(&serde_json::json!({ "session_id": session_id }))?;
        let response = self.send_control_command(OpCode::ResumeSession, &payload)?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::RESUME_SESSION],
        )
    }

    pub fn continue_output(&mut self, pid: u64) -> KernelBridgeResult<TurnControlResult> {
        let payload = serde_json::to_vec(&serde_json::json!({ "pid": pid }))?;
        let response = self.send_control_command(OpCode::ContinueOutput, &payload)?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::CONTINUE_OUTPUT],
        )
    }

    pub fn stop_output(&mut self, pid: u64) -> KernelBridgeResult<TurnControlResult> {
        let payload = serde_json::to_vec(&serde_json::json!({ "pid": pid }))?;
        let response = self.send_control_command(OpCode::StopOutput, &payload)?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::STOP_OUTPUT])
    }

    pub fn terminate_pid(&mut self, pid: u64) -> KernelBridgeResult<()> {
        let payload = pid.to_string();
        let response = self.send_control_command(OpCode::Term, payload.as_bytes())?;
        if response.kind == "+OK" {
            return Ok(());
        }

        let term_err = protocol::decode_protocol_error(&response.code, &response.payload);
        let kill_response = self.send_control_command(OpCode::Kill, payload.as_bytes())?;
        if kill_response.kind == "+OK" {
            return Ok(());
        }

        self.drop_connection();
        let kill_err = protocol::decode_protocol_error(&kill_response.code, &kill_response.payload);
        Err(KernelBridgeError::KernelRejected {
            code: "TERM_KILL_FAILED".to_string(),
            message: format!("TERM failed: {}; KILL failed: {}", term_err, kill_err),
        })
    }

    pub fn shutdown(&mut self) -> KernelBridgeResult<String> {
        let response = self.send_control_command(OpCode::Shutdown, &[])?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        let message = self.decode_response::<ControlMessage>(
            &response.payload,
            &[agentic_protocol::schema::SHUTDOWN],
        )?;
        self.drop_connection();
        Ok(message.message)
    }

    fn ensure_connection(&mut self) -> KernelBridgeResult<&mut TcpStream> {
        if self.stream.is_none() {
            let mut stream = TcpStream::connect(&self.addr)?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            stream.set_write_timeout(Some(Duration::from_secs(5)))?;

            self.authenticate(&mut stream)?;
            protocol::negotiate_hello(&mut stream)?;
            self.stream = Some(stream);
        }

        self.stream
            .as_mut()
            .ok_or(KernelBridgeError::ConnectionUnavailable)
    }

    fn send_control_command(
        &mut self,
        opcode: OpCode,
        payload: &[u8],
    ) -> KernelBridgeResult<protocol::ControlFrame> {
        let timeout = command_timeout(opcode);
        let response = {
            let stream = self.ensure_connection()?;
            protocol::send_command(stream, opcode, "1", payload)
                .and_then(|_| protocol::read_single_frame(stream, timeout))
        };
        match response {
            Ok(frame) => Ok(frame),
            Err(err) => {
                self.drop_connection();
                Err(err)
            }
        }
    }

    fn decode_response<T: serde::de::DeserializeOwned>(
        &mut self,
        payload: &[u8],
        expected_schema_ids: &[&str],
    ) -> KernelBridgeResult<T> {
        match protocol::decode_protocol_data_with_schema(payload, expected_schema_ids) {
            Ok(value) => Ok(value),
            Err(err) => {
                self.drop_connection();
                Err(err)
            }
        }
    }

    fn authenticate(&self, stream: &mut TcpStream) -> KernelBridgeResult<()> {
        let token = load_token(&self.workspace_root)?;
        if token.is_empty() {
            return Ok(());
        }

        protocol::send_command(stream, OpCode::Auth, "1", token.as_bytes())?;
        let response = protocol::read_single_frame(stream, Duration::from_secs(5))?;
        if response.kind != "+OK" {
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        Ok(())
    }

    fn drop_connection(&mut self) {
        self.stream = None;
    }
}

fn command_timeout(opcode: OpCode) -> Duration {
    match opcode {
        OpCode::Load => Duration::from_secs(15 * 60),
        _ => Duration::from_secs(5),
    }
}

fn load_token(workspace_root: &Path) -> KernelBridgeResult<String> {
    let token_path = kernel_token_path(workspace_root);
    match fs::read_to_string(token_path) {
        Ok(token) => Ok(token.trim().to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err.into()),
    }
}

fn map_process_to_session(process: PidStatusResponse) -> AgentSessionSummary {
    let status = match process.state.as_str() {
        "Running" | "WaitingForSyscall" | "InFlight" | "AwaitingRemoteResponse" => "running",
        "Parked" => "swapped",
        _ => "idle",
    }
    .to_string();

    let context_strategy = process
        .context
        .as_ref()
        .map(|context| context.context_strategy.clone())
        .unwrap_or_else(|| "sliding_window".to_string());

    let prompt_preview = if let Some(context) = process.context.as_ref() {
        format!(
            "workload={} | backend={} | slot={} [{} / {}] | context={}/{} tokens | strategy={}",
            process.workload,
            process.backend_class.as_deref().unwrap_or("unknown"),
            process
                .context_slot_id
                .map(|slot_id| slot_id.to_string())
                .unwrap_or_else(|| "none".to_string()),
            process
                .resident_slot_policy
                .as_deref()
                .unwrap_or("unmanaged"),
            process.resident_slot_state.as_deref().unwrap_or("unbound"),
            context.context_tokens_used,
            context.context_window_size,
            context.context_strategy
        )
    } else {
        format!(
            "workload={} | backend={} | slot={} [{} / {}] | no context snapshot available",
            process.workload,
            process.backend_class.as_deref().unwrap_or("unknown"),
            process
                .context_slot_id
                .map(|slot_id| slot_id.to_string())
                .unwrap_or_else(|| "none".to_string()),
            process
                .resident_slot_policy
                .as_deref()
                .unwrap_or("unmanaged"),
            process.resident_slot_state.as_deref().unwrap_or("unbound"),
        )
    };

    AgentSessionSummary {
        session_id: process.session_id.clone(),
        pid: process.pid,
        active_pid: Some(process.pid),
        last_pid: Some(process.pid),
        title: format!("{} / PID {}", process.workload, process.pid),
        prompt_preview,
        status,
        runtime_state: Some(process.state.clone()),
        uptime_label: format_duration(process.elapsed_secs),
        tokens_label: format_tokens(process.tokens_generated),
        context_strategy,
        runtime_id: None,
        runtime_label: process
            .backend_id
            .as_ref()
            .map(|backend_id| format!("{backend_id} · {}", process.workload)),
        backend_class: process.backend_class.clone(),
        orchestration_id: process.orchestration_id,
        orchestration_task_id: process.orchestration_task_id,
    }
}

fn map_orchestration_to_summary(orchestration: OrchSummaryResponse) -> LobbyOrchestrationSummary {
    LobbyOrchestrationSummary {
        orchestration_id: orchestration.orchestration_id,
        total: orchestration.total,
        completed: orchestration.completed,
        running: orchestration.running,
        pending: orchestration.pending,
        failed: orchestration.failed,
        skipped: orchestration.skipped,
        finished: orchestration.finished,
        elapsed_label: format_duration(orchestration.elapsed_secs),
        policy: orchestration.policy,
    }
}

fn map_orchestration_status_to_workspace_snapshot(
    orchestration: OrchStatusResponse,
    task_id: Option<String>,
) -> WorkspaceOrchestrationSnapshot {
    WorkspaceOrchestrationSnapshot {
        orchestration_id: orchestration.orchestration_id,
        task_id: task_id.unwrap_or_default(),
        total: orchestration.total,
        completed: orchestration.completed,
        running: orchestration.running,
        pending: orchestration.pending,
        failed: orchestration.failed,
        skipped: orchestration.skipped,
        finished: orchestration.finished,
        elapsed_secs: orchestration.elapsed_secs,
        policy: orchestration.policy,
        tasks: orchestration
            .tasks
            .into_iter()
            .map(|task| WorkspaceOrchestrationTask {
                task: task.task,
                status: task.status,
                pid: task.pid,
            })
            .collect(),
    }
}

fn map_pid_status_to_workspace_snapshot(
    process: PidStatusResponse,
    orchestration: Option<WorkspaceOrchestrationSnapshot>,
    _orchestration_fetch_error: Option<String>,
) -> WorkspaceSnapshot {
    let context = process
        .context
        .as_ref()
        .map(|context| WorkspaceContextSnapshot {
            context_strategy: context.context_strategy.clone(),
            context_tokens_used: context.context_tokens_used,
            context_window_size: context.context_window_size,
            context_compressions: context.context_compressions,
            context_retrieval_hits: context.context_retrieval_hits,
            context_retrieval_requests: context.context_retrieval_requests,
            context_retrieval_misses: context.context_retrieval_misses,
            context_retrieval_candidates_scored: context.context_retrieval_candidates_scored,
            context_retrieval_segments_selected: context.context_retrieval_segments_selected,
            last_retrieval_candidates_scored: context.last_retrieval_candidates_scored,
            last_retrieval_segments_selected: context.last_retrieval_segments_selected,
            last_retrieval_latency_ms: context.last_retrieval_latency_ms,
            last_retrieval_top_score: context.last_retrieval_top_score,
            last_compaction_reason: context.last_compaction_reason.clone(),
            last_summary_ts: context.last_summary_ts.clone(),
            context_segments: context.context_segments,
            episodic_segments: context.episodic_segments,
            episodic_tokens: context.episodic_tokens,
            retrieve_top_k: context.retrieve_top_k,
            retrieve_candidate_limit: context.retrieve_candidate_limit,
            retrieve_max_segment_chars: context.retrieve_max_segment_chars,
            retrieve_min_score: context.retrieve_min_score,
        });

    WorkspaceSnapshot {
        session_id: process.session_id,
        pid: process.pid,
        active_pid: Some(process.pid),
        last_pid: Some(process.pid),
        title: format!("{} / PID {}", process.workload, process.pid),
        runtime_id: None,
        runtime_label: process
            .backend_id
            .as_ref()
            .map(|backend_id| format!("{backend_id} · {}", process.workload)),
        state: process.state,
        workload: process.workload,
        owner_id: (process.owner_id != 0).then_some(process.owner_id),
        tool_caller: Some(process.tool_caller),
        index_pos: Some(process.index_pos),
        priority: (!process.priority.trim().is_empty()).then_some(process.priority),
        quota_tokens: Some(process.quota_tokens),
        quota_syscalls: Some(process.quota_syscalls),
        context_slot_id: process.context_slot_id,
        resident_slot_policy: process.resident_slot_policy,
        resident_slot_state: process.resident_slot_state,
        resident_slot_snapshot_path: process.resident_slot_snapshot_path,
        backend_id: process.backend_id,
        backend_class: process.backend_class,
        backend_capabilities: process.backend_capabilities,
        accounting: process.session_accounting,
        permissions: Some(process.permissions),
        tokens_generated: process.tokens_generated,
        syscalls_used: process.syscalls_used,
        elapsed_secs: process.elapsed_secs,
        tokens: process.tokens,
        max_tokens: process.max_tokens,
        orchestration,
        context,
        pending_human_request: process.pending_human_request.map(|request| {
            WorkspaceHumanInputRequest {
                request_id: request.request_id,
                kind: request.kind,
                question: request.question,
                details: request.details,
                choices: request.choices,
                allow_free_text: request.allow_free_text,
                placeholder: request.placeholder,
                requested_at_ms: request.requested_at_ms,
            }
        }),
        audit_events: Vec::new(),
    }
}

fn merge_live_session_summary(
    persisted: Option<AgentSessionSummary>,
    live: AgentSessionSummary,
) -> AgentSessionSummary {
    let Some(persisted) = persisted else {
        return live;
    };

    AgentSessionSummary {
        session_id: live.session_id,
        pid: live.pid,
        active_pid: live.active_pid.or(persisted.active_pid),
        last_pid: live.last_pid.or(persisted.last_pid),
        title: if persisted.title.trim().is_empty() {
            live.title
        } else {
            persisted.title
        },
        prompt_preview: if persisted.prompt_preview.trim().is_empty() {
            live.prompt_preview
        } else {
            persisted.prompt_preview
        },
        status: live.status,
        runtime_state: live.runtime_state.or(persisted.runtime_state),
        uptime_label: live.uptime_label,
        tokens_label: live.tokens_label,
        context_strategy: if persisted.context_strategy.trim().is_empty() {
            live.context_strategy
        } else {
            persisted.context_strategy
        },
        runtime_id: persisted.runtime_id.or(live.runtime_id),
        runtime_label: persisted.runtime_label.or(live.runtime_label),
        backend_class: live.backend_class.or(persisted.backend_class),
        orchestration_id: live.orchestration_id,
        orchestration_task_id: live.orchestration_task_id,
    }
}

fn format_duration(elapsed_secs: f64) -> String {
    let seconds = elapsed_secs.max(0.0).round() as u64;
    if seconds >= 3600 {
        format!("{}h", seconds / 3600)
    } else if seconds >= 60 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}s", seconds)
    }
}

fn format_tokens(tokens: usize) -> String {
    if tokens >= 1000 {
        format!("{:.1}k", (tokens as f64) / 1000.0)
    } else {
        tokens.to_string()
    }
}
