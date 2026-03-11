pub use agentic_protocol::{
    schema, validate_content_length, CommandHeader, ControlErrorCode, HelloRequest, HelloResponse,
    OpCode, ProtocolEnvelope, ProtocolEnvelopeError, PROTOCOL_VERSION_V1,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::str;

use crate::config::kernel_config;
use crate::errors::ProtocolError;
use crate::transport::Client;

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

pub fn response_err_code_typed(code: ControlErrorCode, msg: &str) -> Vec<u8> {
    response_err_code(code.as_str(), msg)
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
            Err(err) => {
                response_err_code_typed(ControlErrorCode::ProtocolSerialize, &err.to_string())
            }
        }
    } else if let Some(legacy_payload) = legacy_payload {
        response_ok_code(code, legacy_payload)
    } else {
        match serde_json::to_string(data) {
            Ok(json) => response_ok_code(code, &json),
            Err(err) => {
                response_err_code_typed(ControlErrorCode::ProtocolSerialize, &err.to_string())
            }
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
            Err(err) => {
                response_err_code_typed(ControlErrorCode::ProtocolSerialize, &err.to_string())
            }
        }
    } else {
        response_err_code(code, message)
    }
}

pub fn response_protocol_err_typed(
    client: &Client,
    request_id: &str,
    code: ControlErrorCode,
    schema_id: &str,
    message: &str,
) -> Vec<u8> {
    response_protocol_err(client, request_id, code.as_str(), schema_id, message)
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
    if !supported_versions
        .iter()
        .any(|version| version == PROTOCOL_VERSION_V1)
    {
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
    response_data_with_code("raw", data)
}

pub fn response_data_with_code(code: &str, data: &[u8]) -> Vec<u8> {
    let header = format!("DATA {} {}\r\n", code, data.len());
    let mut vec = header.into_bytes();
    vec.extend_from_slice(data);
    vec
}

#[cfg(test)]
mod tests {
    use agentic_protocol::MAX_CONTENT_LENGTH;

    use super::{
        handle_hello, response_err_code, response_ok_code, stable_capabilities, CommandHeader,
        OpCode,
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

        let register_tool =
            CommandHeader::parse("REGISTER_TOOL 1 2").expect("REGISTER_TOOL should parse");
        assert!(matches!(register_tool.opcode, OpCode::RegisterTool));

        let unregister_tool =
            CommandHeader::parse("UNREGISTER_TOOL 1 2").expect("UNREGISTER_TOOL should parse");
        assert!(matches!(unregister_tool.opcode, OpCode::UnregisterTool));

        let term = CommandHeader::parse("TERM 1 1").expect("TERM should parse");
        assert!(matches!(term.opcode, OpCode::Term));

        let kill = CommandHeader::parse("KILL 1 1").expect("KILL should parse");
        assert!(matches!(kill.opcode, OpCode::Kill));

        let shutdown = CommandHeader::parse("SHUTDOWN 1 0").expect("SHUTDOWN should parse");
        assert!(matches!(shutdown.opcode, OpCode::Shutdown));

        let continue_output =
            CommandHeader::parse("CONTINUE_OUTPUT 1 9").expect("CONTINUE_OUTPUT should parse");
        assert!(matches!(continue_output.opcode, OpCode::ContinueOutput));

        let stop_output =
            CommandHeader::parse("STOP_OUTPUT 1 9").expect("STOP_OUTPUT should parse");
        assert!(matches!(stop_output.opcode, OpCode::StopOutput));
    }

    #[test]
    fn parse_extended_model_opcodes() {
        let list = CommandHeader::parse("LIST_MODELS 1 0").expect("LIST_MODELS should parse");
        assert!(matches!(list.opcode, OpCode::ListModels));

        let select = CommandHeader::parse("SELECT_MODEL 1 7").expect("SELECT_MODEL should parse");
        assert!(matches!(select.opcode, OpCode::SelectModel));

        let info = CommandHeader::parse("MODEL_INFO 1 0").expect("MODEL_INFO should parse");
        assert!(matches!(info.opcode, OpCode::ModelInfo));

        let diag = CommandHeader::parse("BACKEND_DIAG 1 0").expect("BACKEND_DIAG should parse");
        assert!(matches!(diag.opcode, OpCode::BackendDiag));

        let set_gen = CommandHeader::parse("SET_GEN 1 10").expect("SET_GEN should parse");
        assert!(matches!(set_gen.opcode, OpCode::SetGen));

        let get_gen = CommandHeader::parse("GET_GEN 1 0").expect("GET_GEN should parse");
        assert!(matches!(get_gen.opcode, OpCode::GetGen));

        let send_input = CommandHeader::parse("SEND_INPUT 1 32").expect("SEND_INPUT should parse");
        assert!(matches!(send_input.opcode, OpCode::SendInput));
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

        let list_tools = CommandHeader::parse("LIST_TOOLS 1 0").expect("LIST_TOOLS should parse");
        assert!(matches!(list_tools.opcode, OpCode::ListTools));

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
