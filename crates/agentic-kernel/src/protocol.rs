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
#[path = "tests/protocol.rs"]
mod tests;
