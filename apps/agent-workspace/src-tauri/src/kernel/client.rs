use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use agentic_control_models::{
    ControlMessage, LoadModelResult, ModelCatalogSnapshot, OrchStatusResponse,
    OrchSummaryResponse, OrchestrateResult, PidStatusResponse, SelectModelResult,
    SendInputResult, StatusResponse, TurnControlResult,
};
use agentic_protocol::OpCode;

use super::auth::kernel_token_path;
use super::error::{KernelBridgeError, KernelBridgeResult};
use super::mapping::{humanize_kernel_event, make_audit_event};
use super::protocol;
use crate::models::kernel::{
    AgentSessionSummary, LobbyOrchestrationSummary, LobbySnapshot, WorkspaceContextSnapshot,
    WorkspaceOrchestrationSnapshot, WorkspaceOrchestrationTask, WorkspaceSnapshot,
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
        match self.fetch_status() {
            Ok(status) => LobbySnapshot {
                connected: true,
                selected_model_id: status.model.selected_model_id,
                loaded_model_id: status.model.loaded_model_id,
                orchestrations: status
                    .orchestrations
                    .active_orchestrations
                    .into_iter()
                    .map(map_orchestration_to_summary)
                    .collect(),
                sessions: status
                    .processes
                    .active_processes
                    .into_iter()
                    .map(map_process_to_session)
                    .collect(),
                error: None,
            },
            Err(err) => LobbySnapshot {
                connected: false,
                selected_model_id: String::new(),
                loaded_model_id: String::new(),
                orchestrations: Vec::new(),
                sessions: Vec::new(),
                error: Some(err.to_string()),
            },
        }
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
        let (orchestration, orchestration_fetch_error) = if let Some(orch_id) =
            status.orchestration_id
        {
            match self.fetch_orchestration_status(orch_id, status.orchestration_task_id.clone()) {
                Ok(snapshot) => (Some(snapshot), None),
                Err(err) => (None, Some(err)),
            }
        } else {
            (None, None)
        };
        Ok(map_pid_status_to_workspace_snapshot(
            status,
            orchestration,
            orchestration_fetch_error.map(|err| err.to_string()),
        ))
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

    fn fetch_orchestration_status(
        &mut self,
        orch_id: u64,
        task_id: Option<String>,
    ) -> KernelBridgeResult<WorkspaceOrchestrationSnapshot> {
        let payload = format!("orch:{orch_id}");
        let response = self.send_control_command(OpCode::Status, payload.as_bytes())?;

        if response.kind != "+OK" {
            self.drop_connection();
            return Err(protocol::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        let orchestration = self.decode_response::<OrchStatusResponse>(
            &response.payload,
            &[agentic_protocol::schema::ORCH_STATUS],
        )?;
        Ok(WorkspaceOrchestrationSnapshot {
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
        })
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

        self.decode_response(&response.payload, &[agentic_protocol::schema::CONTINUE_OUTPUT])
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
        "Running" | "WaitingForSyscall" | "InFlight" => "running",
        "WaitingForMemory" => "swapped",
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
            "workload={} | context={}/{} tokens | strategy={}",
            process.workload,
            context.context_tokens_used,
            context.context_window_size,
            context.context_strategy
        )
    } else {
        format!(
            "workload={} | no context snapshot available",
            process.workload
        )
    };

    AgentSessionSummary {
        session_id: format!("pid-{}", process.pid),
        pid: process.pid,
        title: format!("{} / PID {}", process.workload, process.pid),
        prompt_preview,
        status,
        uptime_label: format_duration(process.elapsed_secs),
        tokens_label: format_tokens(process.tokens_generated),
        context_strategy,
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

fn map_pid_status_to_workspace_snapshot(
    process: PidStatusResponse,
    orchestration: Option<WorkspaceOrchestrationSnapshot>,
    orchestration_fetch_error: Option<String>,
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
            last_compaction_reason: context.last_compaction_reason.clone(),
            last_summary_ts: context.last_summary_ts.clone(),
            context_segments: context.context_segments,
        });

    let mut audit_events = Vec::new();
    if let Some(context) = process.context.as_ref() {
        audit_events.push(make_audit_event(
            "status",
            "Context snapshot",
            humanize_kernel_event(&format!(
                "strategy={} tokens={}/{} segments={}",
                context.context_strategy,
                context.context_tokens_used,
                context.context_window_size,
                context.context_segments
            )),
        ));
        if let Some(reason) = context.last_compaction_reason.as_ref() {
            audit_events.push(make_audit_event(
                "compaction",
                "Compaction event",
                humanize_kernel_event(reason),
            ));
        }
        if let Some(summary_ts) = context.last_summary_ts.as_ref() {
            audit_events.push(make_audit_event(
                "summary",
                "Summary checkpoint",
                humanize_kernel_event(&format!("last_summary_ts={summary_ts}")),
            ));
        }
    }
    audit_events.push(make_audit_event(
        "runtime",
        "Runtime state",
        humanize_kernel_event(&format!(
            "workload={} state={} elapsed={}s",
            process.workload,
            process.state,
            process.elapsed_secs.max(0.0).round() as u64
        )),
    ));
    if let (Some(orch_id), Some(task_id)) = (
        process.orchestration_id,
        process.orchestration_task_id.as_ref(),
    ) {
        audit_events.push(make_audit_event(
            "orchestration",
            "Orchestrated task",
            format!("orch_id={} task={}", orch_id, task_id),
        ));
    }
    if let Some(err) = orchestration_fetch_error {
        audit_events.push(make_audit_event(
            "orchestration",
            "Orchestration snapshot degraded",
            err,
        ));
    }

    WorkspaceSnapshot {
        session_id: format!("pid-{}", process.pid),
        pid: process.pid,
        state: process.state,
        workload: process.workload,
        tokens_generated: process.tokens_generated,
        syscalls_used: process.syscalls_used,
        elapsed_secs: process.elapsed_secs,
        tokens: process.tokens,
        max_tokens: process.max_tokens,
        orchestration,
        context,
        audit_events,
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
