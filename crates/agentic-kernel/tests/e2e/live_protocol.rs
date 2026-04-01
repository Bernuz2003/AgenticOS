use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection};
use serde_json::json;

#[derive(Debug)]
struct Frame {
    kind: String,
    code: String,
    payload: Vec<u8>,
}

#[derive(Debug)]
struct ProtocolCapture {
    exec_session_id: String,
    frames: Vec<Frame>,
    data_text: String,
    persisted_messages: Vec<(String, String, String)>,
}

#[test]
#[ignore = "requires a running kernel TCP endpoint and a loaded local Qwen runtime"]
fn live_protocol_rigid_prompt_preserves_canonical_tool_marker() {
    let capture = run_protocol_probe(
        r#"Rispondi esattamente con TOOL:calc {"expression":"1847*23"} e nulla altro."#,
    )
    .expect("run protocol probe");

    eprintln!("exec_session_id = {}", capture.exec_session_id);
    for (index, frame) in capture.frames.iter().enumerate() {
        let text = String::from_utf8_lossy(&frame.payload);
        eprintln!(
            "frame[{index}] kind={} code={} payload={:?}",
            frame.kind, frame.code, text
        );
    }
    eprintln!("data_text = {:?}", capture.data_text);
    eprintln!("persisted_messages = {:#?}", capture.persisted_messages);

    assert!(!capture.data_text.contains("TOTOOLOL"));
    assert!(capture
        .persisted_messages
        .iter()
        .all(|(_, _, content)| !content.contains("TOTOOLOL")));
    assert!(
        capture.data_text.contains("TOOL:")
            || capture
                .persisted_messages
                .iter()
                .any(|(role, kind, content)| {
                    role == "system"
                        && kind == "invocation"
                        && content.contains(r#""command":"TOOL:calc {\"expression\":\"1847*23\"}""#)
                })
    );
}

#[test]
#[ignore = "requires a running kernel TCP endpoint and a loaded local Qwen runtime"]
fn live_protocol_natural_prompt_preserves_canonical_tool_marker() {
    let capture = run_protocol_probe(
        "Usa il tool calc per moltiplicare 1847*23. Rispondi solo con la tool invocation corretta.",
    )
    .expect("run protocol probe");

    eprintln!("exec_session_id = {}", capture.exec_session_id);
    eprintln!("data_text = {:?}", capture.data_text);
    eprintln!("persisted_messages = {:#?}", capture.persisted_messages);

    assert!(!capture.data_text.contains("TOTOOLOL"));
    assert!(capture
        .persisted_messages
        .iter()
        .all(|(_, _, content)| !content.contains("TOTOOLOL")));
    assert!(
        capture.data_text.contains("TOOL:")
            || capture
                .persisted_messages
                .iter()
                .any(|(role, kind, content)| {
                    role == "system"
                        && kind == "invocation"
                        && content.contains(r#""command":"TOOL:"#)
                })
    );
}

fn run_protocol_probe(prompt: &str) -> Result<ProtocolCapture, String> {
    let workspace_root = repository_root()?;
    let token = fs::read_to_string(workspace_root.join("workspace/.kernel_token"))
        .map_err(|err| format!("read kernel token: {err}"))?
        .trim()
        .to_string();
    if token.is_empty() {
        return Err("workspace/.kernel_token is empty".to_string());
    }

    let host = std::env::var("AGENTICOS_LIVE_PROTOCOL_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("AGENTICOS_LIVE_PROTOCOL_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(6380);
    let model = std::env::var("AGENTICOS_LIVE_PROTOCOL_MODEL")
        .unwrap_or_else(|_| "qwen3.5-9b/Qwen3.5-9B-Q4_K_M".into());

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|err| format!("parse kernel address: {err}"))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|err| format!("connect kernel tcp: {err}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|err| format!("set read timeout: {err}"))?;
    let mut buffer = Vec::new();

    send_and_expect_ok(&mut stream, &mut buffer, "AUTH", token.as_bytes())?;
    send_and_expect_ok(
        &mut stream,
        &mut buffer,
        "HELLO",
        &serde_json::to_vec(&json!({
            "supported_versions": ["v1"],
            "required_capabilities": [],
        }))
        .expect("HELLO payload"),
    )?;
    send_and_expect_ok(&mut stream, &mut buffer, "SELECT_MODEL", model.as_bytes())?;
    send_and_expect_ok(&mut stream, &mut buffer, "LOAD", model.as_bytes())?;
    let exec = send_and_expect_ok(&mut stream, &mut buffer, "EXEC", prompt.as_bytes())?;
    let exec_session_id = extract_session_id(&exec.payload)?;

    let mut frames = Vec::new();
    let mut idle_timeouts = 0usize;
    let mut data_text = String::new();
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut saw_data = false;
    while Instant::now() < deadline {
        match recv_frame(&mut stream, &mut buffer)? {
            Some(frame) => {
                idle_timeouts = 0;
                if frame.kind == "DATA" {
                    if let Ok(text) = String::from_utf8(frame.payload.clone()) {
                        data_text.push_str(&text);
                    }
                    saw_data = true;
                }
                frames.push(frame);
            }
            None => {
                idle_timeouts += 1;
                let allowed_idle_timeouts = if saw_data { 3 } else { 40 };
                if idle_timeouts >= allowed_idle_timeouts {
                    break;
                }
            }
        }
    }

    let persisted_messages = load_session_messages(&exec_session_id)?;

    Ok(ProtocolCapture {
        exec_session_id,
        frames,
        data_text,
        persisted_messages,
    })
}

fn repository_root() -> Result<PathBuf, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .map(PathBuf::from)
        .ok_or_else(|| "resolve repository root".to_string())
}

fn live_protocol_db_path() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("AGENTICOS_LIVE_PROTOCOL_DB_PATH") {
        return Ok(PathBuf::from(path));
    }
    Ok(repository_root()?.join("workspace/agenticos.db"))
}

fn encode_command(opcode: &str, payload: &[u8]) -> Vec<u8> {
    let mut framed = format!("{opcode} 1 {}\n", payload.len()).into_bytes();
    framed.extend_from_slice(payload);
    framed
}

fn send_and_expect_ok(
    stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
    opcode: &str,
    payload: &[u8],
) -> Result<Frame, String> {
    stream
        .write_all(&encode_command(opcode, payload))
        .map_err(|err| format!("write {opcode}: {err}"))?;
    let deadline = Instant::now()
        + match opcode {
            "LOAD" => Duration::from_secs(180),
            _ => Duration::from_secs(15),
        };
    let frame = loop {
        match recv_frame(stream, buffer)? {
            Some(frame) => break frame,
            None if Instant::now() < deadline => continue,
            None => return Err(format!("timed out waiting for {opcode} response")),
        }
    };
    if frame.kind != "+OK" {
        return Err(format!(
            "{opcode} failed with {} {}: {}",
            frame.kind,
            frame.code,
            String::from_utf8_lossy(&frame.payload)
        ));
    }
    Ok(frame)
}

fn recv_frame(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> Result<Option<Frame>, String> {
    loop {
        if let Some(frame) = try_parse_frame(buffer)? {
            return Ok(Some(frame));
        }

        let mut chunk = [0_u8; 4096];
        match stream.read(&mut chunk) {
            Ok(0) => return Err("kernel closed the connection".to_string()),
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(None);
            }
            Err(err) => return Err(format!("read frame: {err}")),
        }
    }
}

fn try_parse_frame(buffer: &mut Vec<u8>) -> Result<Option<Frame>, String> {
    let Some(header_end) = buffer.windows(2).position(|window| window == b"\r\n") else {
        return Ok(None);
    };

    let header = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut parts = header.split_whitespace();
    let kind = parts
        .next()
        .ok_or_else(|| format!("malformed frame header: {header:?}"))?
        .to_string();
    let code = parts
        .next()
        .ok_or_else(|| format!("malformed frame header: {header:?}"))?
        .to_string();
    let payload_len = parts
        .next()
        .ok_or_else(|| format!("malformed frame header: {header:?}"))?
        .parse::<usize>()
        .map_err(|err| format!("invalid frame length in {header:?}: {err}"))?;
    let frame_end = header_end + 2 + payload_len;
    if buffer.len() < frame_end {
        return Ok(None);
    }

    let payload = buffer[header_end + 2..frame_end].to_vec();
    buffer.drain(..frame_end);
    Ok(Some(Frame {
        kind,
        code,
        payload,
    }))
}

fn extract_session_id(payload: &[u8]) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_slice(payload).map_err(|err| format!("parse EXEC response: {err}"))?;
    value
        .get("data")
        .and_then(|data| data.get("session_id"))
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| "missing session_id in EXEC response".to_string())
}

fn load_session_messages(session_id: &str) -> Result<Vec<(String, String, String)>, String> {
    let conn = Connection::open(live_protocol_db_path()?)
        .map_err(|err| format!("open live protocol db: {err}"))?;
    let mut stmt = conn
        .prepare(
            "select role, kind, content from session_messages where session_id = ? order by message_id",
        )
        .map_err(|err| format!("prepare session_messages query: {err}"))?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|err| format!("query session_messages: {err}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("collect session_messages: {err}"))
}
