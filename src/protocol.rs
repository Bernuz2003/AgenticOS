use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::str;

use crate::config::kernel_config;
use crate::errors::ProtocolError;
use crate::transport::Client;

pub const MAX_CONTENT_LENGTH: usize = 8 * 1024 * 1024;
pub const PROTOCOL_VERSION_V1: &str = "v1";

pub mod schema {
    pub const AUTH: &str = "agenticos.control.auth.v1";
    pub const BACKEND_DIAG: &str = "agenticos.control.backend_diag.v1";
    pub const CHECKPOINT: &str = "agenticos.control.checkpoint.v1";
    pub const EXEC: &str = "agenticos.control.exec.v1";
    pub const ERROR: &str = "agenticos.control.error.v1";
    pub const HELLO: &str = "agenticos.control.hello.v1";
    pub const KILL: &str = "agenticos.control.kill.v1";
    pub const LIST_MODELS: &str = "agenticos.control.list_models.v1";
    pub const LOAD: &str = "agenticos.control.load.v1";
    pub const MEMORY_WRITE: &str = "agenticos.control.memw.v1";
    pub const MODEL_INFO: &str = "agenticos.control.model_info.v1";
    pub const ORCHESTRATE: &str = "agenticos.control.orchestrate.v1";
    pub const ORCH_STATUS: &str = "agenticos.control.orch_status.v1";
    pub const PID_STATUS: &str = "agenticos.control.pid_status.v1";
    pub const PING: &str = "agenticos.control.ping.v1";
    pub const RESTORE: &str = "agenticos.control.restore.v1";
    pub const SELECT_MODEL: &str = "agenticos.control.select_model.v1";
    pub const SET_GEN: &str = "agenticos.control.set_gen.v1";
    pub const GET_GEN: &str = "agenticos.control.get_gen.v1";
    pub const SET_PRIORITY: &str = "agenticos.control.set_priority.v1";
    pub const GET_QUOTA: &str = "agenticos.control.get_quota.v1";
    pub const SET_QUOTA: &str = "agenticos.control.set_quota.v1";
    pub const SHUTDOWN: &str = "agenticos.control.shutdown.v1";
    pub const STATUS: &str = "agenticos.control.status.v1";
    pub const TERM: &str = "agenticos.control.term.v1";
    pub const TOOL_INFO: &str = "agenticos.control.tool_info.v1";
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub struct ProtocolEnvelopeError {
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct HelloRequest {
    #[serde(default)]
    pub supported_versions: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct HelloResponse {
    pub negotiated_version: String,
    pub enabled_capabilities: Vec<String>,
    pub legacy_fallback_allowed: bool,
}

#[derive(Debug, Clone)]
pub enum OpCode {
    Hello,          // Negozia versione/capacita protocollo
    Ping,           // Ping-Pong
    Load,           // Carica modello
    Exec,           // Esegui inferenza
    Kill,           // Termina processo immediatamente
    Term,           // Richiede terminazione graceful
    Status,         // Stato kernel/processo
    Shutdown,       // Shutdown kernel
    MemoryWrite,    // Scrivi tensore in VRAM
    ListModels,     // Lista modelli disponibili
    SelectModel,    // Seleziona modello di default
    ModelInfo,      // Mostra info modello
    BackendDiag,    // Diagnostica backend esterno
    SetGen,         // Configura generation params
    GetGen,         // Legge generation params
    SetPriority,    // Imposta priorità processo
    GetQuota,       // Legge quota/accounting processo
    SetQuota,       // Imposta quota processo
    Checkpoint,     // Salva snapshot kernel su disco
    Restore,        // Ripristina stato kernel da disco
    Orchestrate,    // Registra ed esegue un DAG di task
    ToolInfo,       // Descrive tool/syscall disponibili e policy
    Auth,           // Autenticazione client con token
}

#[derive(Debug)]
pub struct CommandHeader {
    pub opcode: OpCode,
    pub agent_id: String,
    pub content_length: usize,
}

impl CommandHeader {
    /// Parsa la riga di intestazione: "VERB AgentID Length"
    /// Esempio: "EXEC coder_01 500"
    pub fn parse(line: &str) -> Result<Self, ProtocolError> {
        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.is_empty() {
            return Err(ProtocolError::EmptyHeader);
        }

        if parts.len() != 3 {
            return Err(ProtocolError::InvalidHeaderFormat);
        }

        let opcode = match parts[0].to_uppercase().as_str() {
            "HELLO" => OpCode::Hello,
            "PING" => OpCode::Ping,
            "LOAD" => OpCode::Load,
            "EXEC" => OpCode::Exec,
            "KILL" => OpCode::Kill,
            "TERM" => OpCode::Term,
            "STATUS" => OpCode::Status,
            "SHUTDOWN" => OpCode::Shutdown,
            "MEMW" => OpCode::MemoryWrite,
            "LIST_MODELS" => OpCode::ListModels,
            "SELECT_MODEL" => OpCode::SelectModel,
            "MODEL_INFO" => OpCode::ModelInfo,
            "BACKEND_DIAG" => OpCode::BackendDiag,
            "SET_GEN" => OpCode::SetGen,
            "GET_GEN" => OpCode::GetGen,
            "SET_PRIORITY" => OpCode::SetPriority,
            "GET_QUOTA" => OpCode::GetQuota,
            "SET_QUOTA" => OpCode::SetQuota,
            "CHECKPOINT" => OpCode::Checkpoint,
            "RESTORE" => OpCode::Restore,
            "ORCHESTRATE" => OpCode::Orchestrate,
            "TOOL_INFO" => OpCode::ToolInfo,
            "AUTH" => OpCode::Auth,
            _ => return Err(ProtocolError::UnknownOpcode(parts[0].to_string())),
        };

        let agent_id = parts[1].to_string();

        let content_length = parts[2]
            .parse::<usize>()
            .map_err(|_| ProtocolError::InvalidContentLength)?;

        validate_content_length(content_length)?;

        Ok(CommandHeader {
            opcode,
            agent_id,
            content_length,
        })
    }
}

pub fn validate_content_length(content_length: usize) -> Result<(), ProtocolError> {
    if content_length > MAX_CONTENT_LENGTH {
        Err(ProtocolError::ContentLengthTooLarge {
            requested: content_length,
            max: MAX_CONTENT_LENGTH,
        })
    } else {
        Ok(())
    }
}

pub fn response_ok_code(code: &str, msg: &str) -> Vec<u8> {
    let payload = msg.as_bytes();
    let mut out = format!("+OK {} {}\r\n", code, payload.len()).into_bytes();
    out.extend_from_slice(payload);
    out
}

pub fn response_err_code(code: &str, msg: &str) -> Vec<u8> {
    let payload = msg.as_bytes();
    let mut out = format!("-ERR {} {}\r\n", code, payload.len()).into_bytes();
    out.extend_from_slice(payload);
    out
}

pub fn response_protocol_ok<T: Serialize>(
    client: &Client,
    request_id: &str,
    code: &str,
    schema_id: &str,
    data: &T,
    legacy_payload: Option<&str>,
) -> Vec<u8> {
    if should_use_protocol_v1(client) {
        let payload = ProtocolEnvelope {
            protocol_version: PROTOCOL_VERSION_V1.to_string(),
            schema_id: schema_id.to_string(),
            request_id: request_id.to_string(),
            ok: true,
            code: code.to_string(),
            data: Some(data),
            error: None,
            warnings: Vec::new(),
        };
        match serde_json::to_string(&payload) {
            Ok(json) => response_ok_code(code, &json),
            Err(err) => response_err_code("PROTOCOL_SERIALIZE", &err.to_string()),
        }
    } else if let Some(legacy_payload) = legacy_payload {
        response_ok_code(code, legacy_payload)
    } else {
        match serde_json::to_string(data) {
            Ok(json) => response_ok_code(code, &json),
            Err(err) => response_err_code("PROTOCOL_SERIALIZE", &err.to_string()),
        }
    }
}

pub fn response_protocol_err(
    client: &Client,
    request_id: &str,
    code: &str,
    schema_id: &str,
    message: &str,
) -> Vec<u8> {
    if should_use_protocol_v1(client) {
        let payload = ProtocolEnvelope::<Value> {
            protocol_version: PROTOCOL_VERSION_V1.to_string(),
            schema_id: schema_id.to_string(),
            request_id: request_id.to_string(),
            ok: false,
            code: code.to_string(),
            data: None,
            error: Some(ProtocolEnvelopeError {
                message: message.to_string(),
            }),
            warnings: Vec::new(),
        };
        match serde_json::to_string(&payload) {
            Ok(json) => response_err_code(code, &json),
            Err(err) => response_err_code("PROTOCOL_SERIALIZE", &err.to_string()),
        }
    } else {
        response_err_code(code, message)
    }
}

pub fn response_protocol_message(
    client: &Client,
    request_id: &str,
    code: &str,
    schema_id: &str,
    message: &str,
    legacy_payload: &str,
) -> Vec<u8> {
    response_protocol_ok(
        client,
        request_id,
        code,
        schema_id,
        &serde_json::json!({ "message": message }),
        Some(legacy_payload),
    )
}

pub fn handle_hello(client: &mut Client, payload: &[u8], request_id: &str) -> Vec<u8> {
    let payload_text = String::from_utf8_lossy(payload).trim().to_string();
    let hello = if payload_text.is_empty() {
        HelloRequest {
            supported_versions: vec![PROTOCOL_VERSION_V1.to_string()],
            required_capabilities: Vec::new(),
        }
    } else {
        match serde_json::from_str::<HelloRequest>(&payload_text) {
            Ok(value) => value,
            Err(err) => {
                return hello_error_response(
                    ProtocolError::InvalidProtocolJson(err.to_string()),
                    request_id,
                    "HELLO_INVALID",
                );
            }
        }
    };

    let supported_versions = if hello.supported_versions.is_empty() {
        vec![PROTOCOL_VERSION_V1.to_string()]
    } else {
        hello.supported_versions
    };
    if !supported_versions.iter().any(|version| version == PROTOCOL_VERSION_V1) {
        return hello_error_response(
            ProtocolError::UnsupportedProtocolVersion(supported_versions.join(",")),
            request_id,
            "VERSION_MISMATCH",
        );
    }

    let enabled_capabilities = stable_capabilities();
    let enabled_set: HashSet<&str> = enabled_capabilities.iter().map(String::as_str).collect();
    for capability in &hello.required_capabilities {
        if !enabled_set.contains(capability.as_str()) {
            return hello_error_response(
                ProtocolError::MissingCapability(capability.clone()),
                request_id,
                "CAPABILITY_MISSING",
            );
        }
    }

    client.negotiated_protocol_version = Some(PROTOCOL_VERSION_V1.to_string());
    client.enabled_capabilities = enabled_capabilities.iter().cloned().collect();

    response_ok_code(
        "HELLO",
        &serde_json::json!({
            "protocol_version": PROTOCOL_VERSION_V1,
            "schema_id": schema::HELLO,
            "request_id": request_id,
            "ok": true,
            "code": "HELLO",
            "data": HelloResponse {
                negotiated_version: PROTOCOL_VERSION_V1.to_string(),
                enabled_capabilities,
                legacy_fallback_allowed: kernel_config().protocol.allow_legacy_fallback,
            },
            "error": null,
            "warnings": [],
        })
        .to_string(),
    )
}

fn hello_error_response(error: ProtocolError, request_id: &str, code: &str) -> Vec<u8> {
    response_err_code(
        code,
        &serde_json::json!({
            "protocol_version": PROTOCOL_VERSION_V1,
            "schema_id": schema::ERROR,
            "request_id": request_id,
            "ok": false,
            "code": code,
            "data": null,
            "error": {"message": error.to_string()},
            "warnings": [],
        })
        .to_string(),
    )
}

pub fn should_use_protocol_v1(client: &Client) -> bool {
    let protocol_cfg = &kernel_config().protocol;

    client
        .negotiated_protocol_version
        .as_deref()
        .map(|version| version == PROTOCOL_VERSION_V1)
        .unwrap_or(false)
        || protocol_cfg.default_contract_v1
        || !protocol_cfg.allow_legacy_fallback
}

pub fn stable_capabilities() -> Vec<String> {
    let mut capabilities = kernel_config().protocol.enabled_capabilities.clone();
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

// Quando risponderemo con Tensori, useremo un Header simile a quello di richiesta
pub fn response_data(data: &[u8]) -> Vec<u8> {
    let header = format!("DATA raw {}\r\n", data.len());
    let mut vec = header.into_bytes();
    vec.extend_from_slice(data);
    vec
}

#[cfg(test)]
mod tests {
    use super::{
        handle_hello, response_err_code, response_ok_code, stable_capabilities, CommandHeader,
        OpCode, MAX_CONTENT_LENGTH,
    };
    use crate::transport::Client;

    fn test_client() -> Client {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let join = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept client");
            stream
        });

        let client_stream = std::net::TcpStream::connect(addr).expect("connect listener");
        let _server_stream = join.join().expect("join accept thread");
        Client::new(mio::net::TcpStream::from_std(client_stream), true)
    }

    #[test]
    fn parse_basic_opcodes() {
        let hello = CommandHeader::parse("HELLO 1 0").expect("HELLO should parse");
        assert!(matches!(hello.opcode, OpCode::Hello));

        let ping = CommandHeader::parse("PING 1 0").expect("PING should parse");
        assert!(matches!(ping.opcode, OpCode::Ping));

        let load = CommandHeader::parse("LOAD 1 10").expect("LOAD should parse");
        assert!(matches!(load.opcode, OpCode::Load));

        let status = CommandHeader::parse("STATUS 1 0").expect("STATUS should parse");
        assert!(matches!(status.opcode, OpCode::Status));

        let term = CommandHeader::parse("TERM 1 1").expect("TERM should parse");
        assert!(matches!(term.opcode, OpCode::Term));

        let kill = CommandHeader::parse("KILL 1 1").expect("KILL should parse");
        assert!(matches!(kill.opcode, OpCode::Kill));

        let shutdown =
            CommandHeader::parse("SHUTDOWN 1 0").expect("SHUTDOWN should parse");
        assert!(matches!(shutdown.opcode, OpCode::Shutdown));
    }

    #[test]
    fn parse_extended_model_opcodes() {
        let list = CommandHeader::parse("LIST_MODELS 1 0").expect("LIST_MODELS should parse");
        assert!(matches!(list.opcode, OpCode::ListModels));

        let select =
            CommandHeader::parse("SELECT_MODEL 1 7").expect("SELECT_MODEL should parse");
        assert!(matches!(select.opcode, OpCode::SelectModel));

        let info = CommandHeader::parse("MODEL_INFO 1 0").expect("MODEL_INFO should parse");
        assert!(matches!(info.opcode, OpCode::ModelInfo));

        let diag = CommandHeader::parse("BACKEND_DIAG 1 0").expect("BACKEND_DIAG should parse");
        assert!(matches!(diag.opcode, OpCode::BackendDiag));

        let set_gen = CommandHeader::parse("SET_GEN 1 10").expect("SET_GEN should parse");
        assert!(matches!(set_gen.opcode, OpCode::SetGen));

        let get_gen = CommandHeader::parse("GET_GEN 1 0").expect("GET_GEN should parse");
        assert!(matches!(get_gen.opcode, OpCode::GetGen));
    }

    #[test]
    fn parse_scheduler_opcodes() {
        let sp = CommandHeader::parse("SET_PRIORITY 1 5").expect("SET_PRIORITY should parse");
        assert!(matches!(sp.opcode, OpCode::SetPriority));

        let gq = CommandHeader::parse("GET_QUOTA 1 2").expect("GET_QUOTA should parse");
        assert!(matches!(gq.opcode, OpCode::GetQuota));

        let sq = CommandHeader::parse("SET_QUOTA 1 20").expect("SET_QUOTA should parse");
        assert!(matches!(sq.opcode, OpCode::SetQuota));
    }

    #[test]
    fn parse_checkpoint_opcodes() {
        let cp = CommandHeader::parse("CHECKPOINT 1 0").expect("CHECKPOINT should parse");
        assert!(matches!(cp.opcode, OpCode::Checkpoint));

        let rs = CommandHeader::parse("RESTORE 1 0").expect("RESTORE should parse");
        assert!(matches!(rs.opcode, OpCode::Restore));
    }

    #[test]
    fn parse_orchestrate_opcode() {
        let o = CommandHeader::parse("ORCHESTRATE agent_1 200").expect("ORCHESTRATE should parse");
        assert!(matches!(o.opcode, OpCode::Orchestrate));
        assert_eq!(o.agent_id, "agent_1");
        assert_eq!(o.content_length, 200);

        let tool_info = CommandHeader::parse("TOOL_INFO 1 0").expect("TOOL_INFO should parse");
        assert!(matches!(tool_info.opcode, OpCode::ToolInfo));
    }

    #[test]
    fn parse_memw_case_insensitive() {
        let memw = CommandHeader::parse("memw 1 4").expect("memw should parse");
        assert!(matches!(memw.opcode, OpCode::MemoryWrite));
    }

    #[test]
    fn parse_invalid_opcode() {
        let err = CommandHeader::parse("WHAT 1 0").expect_err("invalid opcode must fail");
        assert!(err.to_string().contains("Unknown opcode"));
    }

    #[test]
    fn parse_requires_three_tokens() {
        let err = CommandHeader::parse("PING").expect_err("header without fields must fail");
        assert!(err.to_string().contains("Invalid header format"));

        let err =
            CommandHeader::parse("PING 1 0 extra").expect_err("header with extra fields fails");
        assert!(err.to_string().contains("Invalid header format"));
    }

    #[test]
    fn parse_rejects_oversized_payloads() {
        let err = CommandHeader::parse(&format!("EXEC 1 {}", MAX_CONTENT_LENGTH + 1))
            .expect_err("oversized payload must fail");
        assert!(err.to_string().contains("exceeds protocol limit"));
    }

    #[test]
    fn coded_response_format() {
        let ok = String::from_utf8(response_ok_code("PING", "PONG")).expect("utf8 ok");
        assert!(ok.starts_with("+OK PING 4\r\n"));
        assert!(ok.ends_with("PONG"));

        let err = String::from_utf8(response_err_code("BAD_HEADER", "Malformed")).expect("utf8 ok");
        assert!(err.starts_with("-ERR BAD_HEADER 9\r\n"));
        assert!(err.ends_with("Malformed"));
    }

    #[test]
    fn hello_negotiates_protocol_v1() {
        let mut client = test_client();
        let response = String::from_utf8(handle_hello(
            &mut client,
            br#"{"supported_versions":["v1"],"required_capabilities":[]}"#,
            "req-1",
        ))
        .expect("utf8 response");

        assert!(response.starts_with("+OK HELLO "));
        assert!(response.contains("\"protocol_version\":\"v1\""));
        assert_eq!(client.negotiated_protocol_version.as_deref(), Some("v1"));
    }

    #[test]
    fn stable_capabilities_are_non_empty() {
        assert!(!stable_capabilities().is_empty());
    }
}