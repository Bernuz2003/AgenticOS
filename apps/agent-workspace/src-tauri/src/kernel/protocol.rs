use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use agentic_protocol::{
    encode_command, HelloRequest, HelloResponse, OpCode, ProtocolEnvelope, PROTOCOL_VERSION_V1,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::error::{KernelBridgeError, KernelBridgeResult};

pub fn default_protocol_version() -> &'static str {
    PROTOCOL_VERSION_V1
}

pub fn default_hello_request() -> HelloRequest {
    HelloRequest {
        supported_versions: vec![PROTOCOL_VERSION_V1.to_string()],
        required_capabilities: Vec::new(),
    }
}

#[derive(Debug, Clone)]
pub struct ControlFrame {
    pub kind: String,
    pub code: String,
    pub payload: Vec<u8>,
}

pub fn send_command(
    stream: &mut TcpStream,
    opcode: OpCode,
    agent_id: &str,
    payload: &[u8],
) -> KernelBridgeResult<()> {
    let frame = encode_command(opcode, agent_id, payload)?;
    stream.write_all(&frame)?;
    Ok(())
}

pub fn read_single_frame(
    stream: &mut TcpStream,
    timeout: Duration,
) -> KernelBridgeResult<ControlFrame> {
    let started_at = Instant::now();
    let mut buffer = Vec::new();

    stream.set_read_timeout(Some(timeout))?;

    loop {
        if let Some(frame) = consume_first_frame(&mut buffer)? {
            return Ok(frame);
        }

        if started_at.elapsed() >= timeout {
            return Err(KernelBridgeError::TimedOut("kernel response"));
        }

        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(KernelBridgeError::ConnectionClosed);
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

pub fn read_stream_frame(
    stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
    timeout: Duration,
) -> KernelBridgeResult<Option<ControlFrame>> {
    if let Some(frame) = consume_first_frame(buffer)? {
        return Ok(Some(frame));
    }

    stream.set_read_timeout(Some(timeout))?;

    let mut chunk = [0_u8; 4096];
    match stream.read(&mut chunk) {
        Ok(0) => Err(KernelBridgeError::ConnectionClosed),
        Ok(read) => {
            buffer.extend_from_slice(&chunk[..read]);
            consume_first_frame(buffer)
        }
        Err(err)
            if err.kind() == std::io::ErrorKind::WouldBlock
                || err.kind() == std::io::ErrorKind::TimedOut =>
        {
            Ok(None)
        }
        Err(err) => Err(err.into()),
    }
}

fn consume_first_frame(buffer: &mut Vec<u8>) -> KernelBridgeResult<Option<ControlFrame>> {
    let Some(line_end) = buffer.windows(2).position(|window| window == b"\r\n") else {
        return Ok(None);
    };

    let header = String::from_utf8_lossy(&buffer[..line_end]).to_string();
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() < 3 {
        return Err(KernelBridgeError::MalformedResponseHeader);
    }

    let kind = parts[0].to_string();
    let code = parts[1].to_string();
    let payload_len = parts[2]
        .parse::<usize>()
        .map_err(|_| KernelBridgeError::InvalidPayloadLength)?;

    let total_needed = line_end + 2 + payload_len;
    if buffer.len() < total_needed {
        return Ok(None);
    }

    let payload = buffer[line_end + 2..total_needed].to_vec();
    buffer.drain(..total_needed);

    Ok(Some(ControlFrame {
        kind,
        code,
        payload,
    }))
}

pub fn decode_protocol_data<T: DeserializeOwned>(payload: &[u8]) -> KernelBridgeResult<T> {
    decode_protocol_data_with_schema(payload, &[])
}

pub fn decode_protocol_data_with_schema<T: DeserializeOwned>(
    payload: &[u8],
    expected_schema_ids: &[&str],
) -> KernelBridgeResult<T> {
    let text = std::str::from_utf8(payload)?;
    if let Ok(envelope) = serde_json::from_str::<ProtocolEnvelope<T>>(text) {
        if !expected_schema_ids.is_empty()
            && !expected_schema_ids
                .iter()
                .any(|schema_id| *schema_id == envelope.schema_id)
        {
            return Err(KernelBridgeError::UnexpectedSchema {
                received: envelope.schema_id,
                expected: expected_schema_ids.join(", "),
            });
        }
        return envelope.data.ok_or(KernelBridgeError::MissingEnvelopeData);
    }

    if !expected_schema_ids.is_empty() {
        return Err(KernelBridgeError::MissingProtocolEnvelope {
            expected: expected_schema_ids.join(", "),
        });
    }

    Ok(serde_json::from_str::<T>(text)?)
}

pub fn decode_protocol_error(code: &str, payload: &[u8]) -> KernelBridgeError {
    let Ok(text) = std::str::from_utf8(payload) else {
        return KernelBridgeError::KernelRejected {
            code: code.to_string(),
            message: "Kernel returned a non UTF-8 error payload".to_string(),
        };
    };

    if let Ok(envelope) = serde_json::from_str::<ProtocolEnvelope<Value>>(text) {
        if let Some(error) = envelope.error {
            return KernelBridgeError::KernelRejected {
                code: code.to_string(),
                message: error.message,
            };
        }
    }

    KernelBridgeError::KernelRejected {
        code: code.to_string(),
        message: text.to_string(),
    }
}

pub fn negotiate_hello(stream: &mut TcpStream) -> KernelBridgeResult<HelloResponse> {
    let payload = serde_json::to_vec(&default_hello_request())?;
    send_command(stream, OpCode::Hello, "1", &payload)?;
    let response = read_single_frame(stream, Duration::from_secs(5))?;
    if response.kind != "+OK" {
        return Err(decode_protocol_error(&response.code, &response.payload));
    }
    decode_protocol_data::<HelloResponse>(&response.payload)
}

#[cfg(test)]
mod tests {
    use super::{decode_protocol_data_with_schema, decode_protocol_error, KernelBridgeError};

    #[test]
    fn decode_protocol_data_rejects_unexpected_schema() {
        let payload = br#"{"protocol_version":"v1","schema_id":"wrong.schema","request_id":"1","ok":true,"code":"STATUS","data":{"message":"x"},"error":null,"warnings":[]}"#;
        let err =
            decode_protocol_data_with_schema::<serde_json::Value>(payload, &["expected.schema"])
                .expect_err("schema mismatch expected");

        match err {
            KernelBridgeError::UnexpectedSchema { received, expected } => {
                assert_eq!(received, "wrong.schema");
                assert_eq!(expected, "expected.schema");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn decode_protocol_error_preserves_kernel_code() {
        let err = decode_protocol_error(
            "LOAD_BUSY",
            br#"{"protocol_version":"v1","schema_id":"agenticos.control.error.v1","request_id":"1","ok":false,"code":"LOAD_BUSY","data":null,"error":{"message":"Cannot LOAD while 1 live process(es) are still present."},"warnings":[]}"#,
        );

        match err {
            KernelBridgeError::KernelRejected { code, message } => {
                assert_eq!(code, "LOAD_BUSY");
                assert!(message.contains("Cannot LOAD"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
