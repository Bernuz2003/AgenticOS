use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_CONTENT_LENGTH: usize = 8 * 1024 * 1024;
pub const PROTOCOL_VERSION_V1: &str = "v1";

pub mod schema {
    pub const AUTH: &str = "agenticos.control.auth.v1";
    pub const BACKEND_DIAG: &str = "agenticos.control.backend_diag.v1";
    pub const CHECKPOINT: &str = "agenticos.control.checkpoint.v1";
    pub const CONTINUE_OUTPUT: &str = "agenticos.control.continue_output.v1";
    pub const EXEC: &str = "agenticos.control.exec.v1";
    pub const ERROR: &str = "agenticos.control.error.v1";
    pub const GET_GEN: &str = "agenticos.control.get_gen.v1";
    pub const GET_QUOTA: &str = "agenticos.control.get_quota.v1";
    pub const HELLO: &str = "agenticos.control.hello.v1";
    pub const KILL: &str = "agenticos.control.kill.v1";
    pub const LIST_MODELS: &str = "agenticos.control.list_models.v1";
    pub const LIST_TOOLS: &str = "agenticos.control.list_tools.v1";
    pub const LOAD: &str = "agenticos.control.load.v1";
    pub const MEMORY_WRITE: &str = "agenticos.control.memw.v1";
    pub const MODEL_INFO: &str = "agenticos.control.model_info.v1";
    pub const ORCHESTRATE: &str = "agenticos.control.orchestrate.v1";
    pub const ORCH_STATUS: &str = "agenticos.control.orch_status.v1";
    pub const PID_STATUS: &str = "agenticos.control.pid_status.v1";
    pub const PING: &str = "agenticos.control.ping.v1";
    pub const REGISTER_TOOL: &str = "agenticos.control.register_tool.v1";
    pub const RETRY_TASK: &str = "agenticos.control.retry_task.v1";
    pub const RESTORE: &str = "agenticos.control.restore.v1";
    pub const RESUME_SESSION: &str = "agenticos.control.resume_session.v1";
    pub const SCHEDULE_JOB: &str = "agenticos.control.schedule_job.v1";
    pub const SEND_INPUT: &str = "agenticos.control.send_input.v1";
    pub const SELECT_MODEL: &str = "agenticos.control.select_model.v1";
    pub const SET_GEN: &str = "agenticos.control.set_gen.v1";
    pub const SET_PRIORITY: &str = "agenticos.control.set_priority.v1";
    pub const SET_QUOTA: &str = "agenticos.control.set_quota.v1";
    pub const SHUTDOWN: &str = "agenticos.control.shutdown.v1";
    pub const STOP_OUTPUT: &str = "agenticos.control.stop_output.v1";
    pub const STATUS: &str = "agenticos.control.status.v1";
    pub const SUBSCRIBE: &str = "agenticos.control.subscribe.v1";
    pub const TERM: &str = "agenticos.control.term.v1";
    pub const TOOL_INFO: &str = "agenticos.control.tool_info.v1";
    pub const UNREGISTER_TOOL: &str = "agenticos.control.unregister_tool.v1";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControlErrorCode {
    AuthFailed,
    AuthRequired,
    BackendDiag,
    CapabilityRequired,
    CheckpointFailed,
    DriverUnresolved,
    Generic,
    GetQuotaInvalid,
    InFlight,
    ContinueOutputInvalid,
    InvalidPid,
    InvalidSessionState,
    InvalidToolName,
    InvalidToolRegistration,
    InvalidToolUnregistration,
    LoadBusy,
    LoadFailed,
    MemwFailed,
    MemwInvalid,
    MissingModelId,
    MissingPid,
    MissingPrompt,
    MissingToolName,
    ModelNotFound,
    ModelSelector,
    NoModel,
    OrchNotFound,
    OrchestrateInvalid,
    OrchestrateJson,
    RetryTaskInvalid,
    PidNotFound,
    ProtocolSerialize,
    RegisterToolFailed,
    RestoreBusy,
    RestoreFailed,
    ResumeSessionInvalid,
    ScheduleJobInvalid,
    SchedulerLoadFailed,
    SchedulerTargetFailed,
    SendInputInvalid,
    SetPriorityInvalid,
    SetQuotaInvalid,
    SetGenInvalid,
    SpawnFailed,
    StatusInvalid,
    StopOutputInvalid,
    ToolNotFound,
    ToolRegistryMutationForbidden,
    UnregisterToolFailed,
}

impl ControlErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AuthFailed => "AUTH_FAILED",
            Self::AuthRequired => "AUTH_REQUIRED",
            Self::BackendDiag => "BACKEND_DIAG",
            Self::CapabilityRequired => "CAPABILITY_REQUIRED",
            Self::CheckpointFailed => "CHECKPOINT_FAILED",
            Self::DriverUnresolved => "DRIVER_UNRESOLVED",
            Self::Generic => "GENERIC",
            Self::GetQuotaInvalid => "GET_QUOTA_INVALID",
            Self::InFlight => "IN_FLIGHT",
            Self::ContinueOutputInvalid => "CONTINUE_OUTPUT_INVALID",
            Self::InvalidPid => "INVALID_PID",
            Self::InvalidSessionState => "INVALID_SESSION_STATE",
            Self::InvalidToolName => "INVALID_TOOL_NAME",
            Self::InvalidToolRegistration => "INVALID_TOOL_REGISTRATION",
            Self::InvalidToolUnregistration => "INVALID_TOOL_UNREGISTRATION",
            Self::LoadBusy => "LOAD_BUSY",
            Self::LoadFailed => "LOAD_FAILED",
            Self::MemwFailed => "MEMW_FAILED",
            Self::MemwInvalid => "MEMW_INVALID",
            Self::MissingModelId => "MISSING_MODEL_ID",
            Self::MissingPid => "MISSING_PID",
            Self::MissingPrompt => "MISSING_PROMPT",
            Self::MissingToolName => "MISSING_TOOL_NAME",
            Self::ModelNotFound => "MODEL_NOT_FOUND",
            Self::ModelSelector => "MODEL_SELECTOR",
            Self::NoModel => "NO_MODEL",
            Self::OrchNotFound => "ORCH_NOT_FOUND",
            Self::OrchestrateInvalid => "ORCHESTRATE_INVALID",
            Self::OrchestrateJson => "ORCHESTRATE_JSON",
            Self::RetryTaskInvalid => "RETRY_TASK_INVALID",
            Self::PidNotFound => "PID_NOT_FOUND",
            Self::ProtocolSerialize => "PROTOCOL_SERIALIZE",
            Self::RegisterToolFailed => "REGISTER_TOOL_FAILED",
            Self::RestoreBusy => "RESTORE_BUSY",
            Self::RestoreFailed => "RESTORE_FAILED",
            Self::ResumeSessionInvalid => "RESUME_SESSION_INVALID",
            Self::ScheduleJobInvalid => "SCHEDULE_JOB_INVALID",
            Self::SchedulerLoadFailed => "SCHEDULER_LOAD_FAILED",
            Self::SchedulerTargetFailed => "SCHEDULER_TARGET_FAILED",
            Self::SendInputInvalid => "SEND_INPUT_INVALID",
            Self::SetPriorityInvalid => "SET_PRIORITY_INVALID",
            Self::SetQuotaInvalid => "SET_QUOTA_INVALID",
            Self::SetGenInvalid => "SET_GEN_INVALID",
            Self::SpawnFailed => "SPAWN_FAILED",
            Self::StatusInvalid => "STATUS_INVALID",
            Self::StopOutputInvalid => "STOP_OUTPUT_INVALID",
            Self::ToolNotFound => "TOOL_NOT_FOUND",
            Self::ToolRegistryMutationForbidden => "TOOL_REGISTRY_MUTATION_FORBIDDEN",
            Self::UnregisterToolFailed => "UNREGISTER_TOOL_FAILED",
        }
    }
}

impl std::fmt::Display for ControlErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolParseError {
    #[error("Empty header")]
    EmptyHeader,

    #[error("Invalid header format. Expected: <OPCODE> <AGENT_ID> <CONTENT_LENGTH>")]
    InvalidHeaderFormat,

    #[error("Unknown opcode: {0}")]
    UnknownOpcode(String),

    #[error("Invalid content length")]
    InvalidContentLength,

    #[error("Content length {requested} exceeds protocol limit {max}")]
    ContentLengthTooLarge { requested: usize, max: usize },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProtocolEnvelope<T> {
    pub protocol_version: String,
    pub schema_id: String,
    pub request_id: String,
    pub ok: bool,
    pub code: String,
    pub data: Option<T>,
    pub error: Option<ProtocolEnvelopeError>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProtocolEnvelopeError {
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HelloRequest {
    #[serde(default)]
    pub supported_versions: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HelloResponse {
    pub negotiated_version: String,
    pub enabled_capabilities: Vec<String>,
    pub legacy_fallback_allowed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpCode {
    Hello,
    Ping,
    Load,
    Exec,
    SendInput,
    ContinueOutput,
    StopOutput,
    Kill,
    Term,
    Status,
    Shutdown,
    Subscribe,
    MemoryWrite,
    ListModels,
    SelectModel,
    ModelInfo,
    BackendDiag,
    SetGen,
    GetGen,
    SetPriority,
    GetQuota,
    SetQuota,
    Checkpoint,
    Restore,
    ResumeSession,
    ScheduleJob,
    Orchestrate,
    RetryTask,
    ListTools,
    RegisterTool,
    ToolInfo,
    UnregisterTool,
    Auth,
}

impl OpCode {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_uppercase().as_str() {
            "HELLO" => Some(Self::Hello),
            "PING" => Some(Self::Ping),
            "LOAD" => Some(Self::Load),
            "EXEC" => Some(Self::Exec),
            "SEND_INPUT" => Some(Self::SendInput),
            "CONTINUE_OUTPUT" => Some(Self::ContinueOutput),
            "STOP_OUTPUT" => Some(Self::StopOutput),
            "KILL" => Some(Self::Kill),
            "TERM" => Some(Self::Term),
            "STATUS" => Some(Self::Status),
            "SHUTDOWN" => Some(Self::Shutdown),
            "SUBSCRIBE" => Some(Self::Subscribe),
            "MEMW" => Some(Self::MemoryWrite),
            "LIST_MODELS" => Some(Self::ListModels),
            "SELECT_MODEL" => Some(Self::SelectModel),
            "MODEL_INFO" => Some(Self::ModelInfo),
            "BACKEND_DIAG" => Some(Self::BackendDiag),
            "SET_GEN" => Some(Self::SetGen),
            "GET_GEN" => Some(Self::GetGen),
            "SET_PRIORITY" => Some(Self::SetPriority),
            "GET_QUOTA" => Some(Self::GetQuota),
            "SET_QUOTA" => Some(Self::SetQuota),
            "CHECKPOINT" => Some(Self::Checkpoint),
            "RESTORE" => Some(Self::Restore),
            "RESUME_SESSION" => Some(Self::ResumeSession),
            "SCHEDULE_JOB" => Some(Self::ScheduleJob),
            "ORCHESTRATE" => Some(Self::Orchestrate),
            "RETRY_TASK" => Some(Self::RetryTask),
            "LIST_TOOLS" => Some(Self::ListTools),
            "REGISTER_TOOL" => Some(Self::RegisterTool),
            "TOOL_INFO" => Some(Self::ToolInfo),
            "UNREGISTER_TOOL" => Some(Self::UnregisterTool),
            "AUTH" => Some(Self::Auth),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hello => "HELLO",
            Self::Ping => "PING",
            Self::Load => "LOAD",
            Self::Exec => "EXEC",
            Self::SendInput => "SEND_INPUT",
            Self::ContinueOutput => "CONTINUE_OUTPUT",
            Self::StopOutput => "STOP_OUTPUT",
            Self::Kill => "KILL",
            Self::Term => "TERM",
            Self::Status => "STATUS",
            Self::Shutdown => "SHUTDOWN",
            Self::Subscribe => "SUBSCRIBE",
            Self::MemoryWrite => "MEMW",
            Self::ListModels => "LIST_MODELS",
            Self::SelectModel => "SELECT_MODEL",
            Self::ModelInfo => "MODEL_INFO",
            Self::BackendDiag => "BACKEND_DIAG",
            Self::SetGen => "SET_GEN",
            Self::GetGen => "GET_GEN",
            Self::SetPriority => "SET_PRIORITY",
            Self::GetQuota => "GET_QUOTA",
            Self::SetQuota => "SET_QUOTA",
            Self::Checkpoint => "CHECKPOINT",
            Self::Restore => "RESTORE",
            Self::ResumeSession => "RESUME_SESSION",
            Self::ScheduleJob => "SCHEDULE_JOB",
            Self::Orchestrate => "ORCHESTRATE",
            Self::RetryTask => "RETRY_TASK",
            Self::ListTools => "LIST_TOOLS",
            Self::RegisterTool => "REGISTER_TOOL",
            Self::ToolInfo => "TOOL_INFO",
            Self::UnregisterTool => "UNREGISTER_TOOL",
            Self::Auth => "AUTH",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandHeader {
    pub opcode: OpCode,
    pub agent_id: String,
    pub content_length: usize,
}

impl CommandHeader {
    pub fn parse(line: &str) -> Result<Self, ProtocolParseError> {
        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.is_empty() {
            return Err(ProtocolParseError::EmptyHeader);
        }

        if parts.len() != 3 {
            return Err(ProtocolParseError::InvalidHeaderFormat);
        }

        let opcode = OpCode::parse(parts[0])
            .ok_or_else(|| ProtocolParseError::UnknownOpcode(parts[0].to_string()))?;
        let agent_id = parts[1].to_string();
        let content_length = parts[2]
            .parse::<usize>()
            .map_err(|_| ProtocolParseError::InvalidContentLength)?;

        validate_content_length(content_length)?;

        Ok(Self {
            opcode,
            agent_id,
            content_length,
        })
    }

    pub fn encode(&self) -> String {
        format!(
            "{} {} {}",
            self.opcode.as_str(),
            self.agent_id,
            self.content_length
        )
    }
}

pub fn validate_content_length(content_length: usize) -> Result<(), ProtocolParseError> {
    if content_length > MAX_CONTENT_LENGTH {
        Err(ProtocolParseError::ContentLengthTooLarge {
            requested: content_length,
            max: MAX_CONTENT_LENGTH,
        })
    } else {
        Ok(())
    }
}

pub fn encode_command(
    opcode: OpCode,
    agent_id: &str,
    payload: &[u8],
) -> Result<Vec<u8>, ProtocolParseError> {
    validate_content_length(payload.len())?;
    let mut out = format!("{} {} {}\n", opcode.as_str(), agent_id, payload.len()).into_bytes();
    out.extend_from_slice(payload);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{
        encode_command, CommandHeader, ControlErrorCode, OpCode, ProtocolParseError,
        MAX_CONTENT_LENGTH,
    };

    #[test]
    fn parses_command_header() {
        let header = CommandHeader::parse("EXEC agent_1 42").expect("parse header");
        assert_eq!(header.opcode, OpCode::Exec);
        assert_eq!(header.agent_id, "agent_1");
        assert_eq!(header.content_length, 42);
    }

    #[test]
    fn control_error_code_formats_stable_wire_codes() {
        assert_eq!(ControlErrorCode::LoadBusy.as_str(), "LOAD_BUSY");
        assert_eq!(
            ControlErrorCode::StatusInvalid.to_string(),
            "STATUS_INVALID"
        );
        assert_eq!(ControlErrorCode::NoModel.as_str(), "NO_MODEL");
        assert_eq!(
            ControlErrorCode::ContinueOutputInvalid.as_str(),
            "CONTINUE_OUTPUT_INVALID"
        );
    }

    #[test]
    fn rejects_unknown_opcode() {
        let err = CommandHeader::parse("NOPE agent_1 0").expect_err("unknown opcode must fail");
        assert_eq!(err, ProtocolParseError::UnknownOpcode("NOPE".to_string()));
    }

    #[test]
    fn rejects_oversized_payload() {
        let err = encode_command(OpCode::Exec, "agent_1", &vec![0_u8; MAX_CONTENT_LENGTH + 1])
            .expect_err("oversized payload must fail");
        assert!(matches!(
            err,
            ProtocolParseError::ContentLengthTooLarge { .. }
        ));
    }
}
