use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agentic_control_models::{
    ControlMessage, ResumeSessionResult, SendInputResult, TurnControlResult,
};
use agentic_protocol::{
    encode_command, HelloRequest, HelloResponse, OpCode, ProtocolEnvelope, PROTOCOL_VERSION_V1,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

use crate::kernel::auth::kernel_token_path;

#[derive(Debug, Error)]
pub enum KernelBridgeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol framing error: {0}")]
    ProtocolParse(#[from] agentic_protocol::ProtocolParseError),

    #[error("JSON decode error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("UTF-8 decode error: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("Timed out waiting for {0}")]
    TimedOut(&'static str),

    #[error("Kernel connection closed")]
    ConnectionClosed,

    #[error("Malformed kernel response header")]
    MalformedResponseHeader,

    #[error("Invalid payload length in kernel response")]
    InvalidPayloadLength,

    #[error("Kernel payload missing protocol envelope for schema(s): {expected}")]
    MissingProtocolEnvelope { expected: String },

    #[error("Protocol envelope did not contain data")]
    MissingEnvelopeData,

    #[error("Unexpected kernel schema '{received}', expected one of: {expected}")]
    UnexpectedSchema { received: String, expected: String },

    #[error("Kernel returned error {code}: {message}")]
    KernelRejected { code: String, message: String },

    #[error("Kernel connection is not available")]
    ConnectionUnavailable,
}

pub type KernelBridgeResult<T> = Result<T, KernelBridgeError>;

#[derive(Debug, Clone)]
pub struct ControlFrame {
    pub kind: String,
    pub code: String,
    pub payload: Vec<u8>,
}

pub fn default_protocol_version() -> &'static str {
    PROTOCOL_VERSION_V1
}

pub fn default_hello_request() -> HelloRequest {
    HelloRequest {
        supported_versions: vec![PROTOCOL_VERSION_V1.to_string()],
        required_capabilities: Vec::new(),
    }
}

#[derive(Debug)]
pub struct KernelBridge {
    pub(super) addr: String,
    pub(super) workspace_root: PathBuf,
    pub(super) stream: Option<TcpStream>,
}

impl KernelBridge {
    pub fn new(addr: String, workspace_root: PathBuf) -> Self {
        Self {
            addr,
            workspace_root,
            stream: None,
        }
    }

    pub fn ping(&mut self) -> KernelBridgeResult<String> {
        let response = self.send_control_command(OpCode::Ping, &[])?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(decode_protocol_error(&response.code, &response.payload));
        }

        let message = self.decode_response::<ControlMessage>(
            &response.payload,
            &[agentic_protocol::schema::PING],
        )?;
        Ok(message.message)
    }

    pub fn send_input(
        &mut self,
        pid: Option<u64>,
        session_id: Option<&str>,
        prompt: &str,
    ) -> KernelBridgeResult<SendInputResult> {
        let payload = serde_json::to_vec(&serde_json::json!({
            "pid": pid,
            "session_id": session_id,
            "prompt": prompt,
        }))?;
        let response = self.send_control_command(OpCode::SendInput, &payload)?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(decode_protocol_error(&response.code, &response.payload));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::SEND_INPUT])
    }

    pub fn resume_session(&mut self, session_id: &str) -> KernelBridgeResult<ResumeSessionResult> {
        let payload = serde_json::to_vec(&serde_json::json!({ "session_id": session_id }))?;
        let response = self.send_control_command(OpCode::ResumeSession, &payload)?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(decode_protocol_error(&response.code, &response.payload));
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
            return Err(decode_protocol_error(&response.code, &response.payload));
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
            return Err(decode_protocol_error(&response.code, &response.payload));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::STOP_OUTPUT])
    }

    pub fn terminate_pid(&mut self, pid: u64) -> KernelBridgeResult<()> {
        let payload = pid.to_string();
        let response = self.send_control_command(OpCode::Term, payload.as_bytes())?;
        if response.kind == "+OK" {
            return Ok(());
        }

        let term_err = decode_protocol_error(&response.code, &response.payload);
        let kill_response = self.send_control_command(OpCode::Kill, payload.as_bytes())?;
        if kill_response.kind == "+OK" {
            return Ok(());
        }

        self.drop_connection();
        let kill_err = decode_protocol_error(&kill_response.code, &kill_response.payload);
        Err(KernelBridgeError::KernelRejected {
            code: "TERM_KILL_FAILED".to_string(),
            message: format!("TERM failed: {}; KILL failed: {}", term_err, kill_err),
        })
    }

    pub fn shutdown(&mut self) -> KernelBridgeResult<String> {
        let response = self.send_control_command(OpCode::Shutdown, &[])?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(decode_protocol_error(&response.code, &response.payload));
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
            negotiate_hello(&mut stream)?;
            self.stream = Some(stream);
        }

        self.stream
            .as_mut()
            .ok_or(KernelBridgeError::ConnectionUnavailable)
    }

    pub(super) fn send_control_command(
        &mut self,
        opcode: OpCode,
        payload: &[u8],
    ) -> KernelBridgeResult<ControlFrame> {
        let timeout = command_timeout(opcode);
        let response = {
            let stream = self.ensure_connection()?;
            send_command(stream, opcode, "1", payload)
                .and_then(|_| read_single_frame(stream, timeout))
        };
        match response {
            Ok(frame) => Ok(frame),
            Err(err) => {
                self.drop_connection();
                Err(err)
            }
        }
    }

    pub(super) fn decode_response<T: DeserializeOwned>(
        &mut self,
        payload: &[u8],
        expected_schema_ids: &[&str],
    ) -> KernelBridgeResult<T> {
        match decode_protocol_data_with_schema(payload, expected_schema_ids) {
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

        send_command(stream, OpCode::Auth, "1", token.as_bytes())?;
        let response = read_single_frame(stream, Duration::from_secs(5))?;
        if response.kind != "+OK" {
            return Err(decode_protocol_error(&response.code, &response.payload));
        }

        Ok(())
    }

    pub(super) fn drop_connection(&mut self) {
        self.stream = None;
    }
}

fn command_timeout(opcode: OpCode) -> Duration {
    match opcode {
        OpCode::Load => Duration::from_secs(15 * 60),
        OpCode::ReplayCoreDump => Duration::from_secs(30),
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
