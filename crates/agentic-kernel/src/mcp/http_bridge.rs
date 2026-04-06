use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::mcp::bridge::McpBridgeState;
use crate::mcp::models::{McpBridgeErrorResponse, McpBridgeInvocationRequest};

pub(super) fn spawn(
    host: &str,
    port: u16,
    token_header: &str,
    token: &str,
    state: Arc<Mutex<McpBridgeState>>,
) -> Result<(SocketAddr, mpsc::Sender<()>, thread::JoinHandle<()>), String> {
    let listener = TcpListener::bind((host, port))
        .map_err(|err| format!("Failed to bind MCP HTTP bridge on {host}:{port}: {err}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|err| format!("Failed to set MCP bridge listener non-blocking: {err}"))?;
    let listen_addr = listener
        .local_addr()
        .map_err(|err| format!("Failed to query MCP bridge address: {err}"))?;
    let (shutdown_tx, shutdown_rx) = mpsc::channel();
    let token_header = token_header.to_ascii_lowercase();
    let token = token.to_string();

    let handle = thread::Builder::new()
        .name("mcp-http-bridge".to_string())
        .spawn(move || loop {
            if shutdown_rx.try_recv().is_ok() {
                break;
            }

            match listener.accept() {
                Ok((mut stream, _)) => {
                    if let Err(err) = handle_connection(&mut stream, &token_header, &token, &state)
                    {
                        tracing::debug!(%err, "MCP bridge request failed");
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(err) => {
                    tracing::warn!(%err, "MCP bridge accept failed");
                    thread::sleep(Duration::from_millis(50));
                }
            }
        })
        .map_err(|err| format!("Failed to start MCP bridge thread: {err}"))?;

    Ok((listen_addr, shutdown_tx, handle))
}

fn handle_connection(
    stream: &mut TcpStream,
    token_header: &str,
    token: &str,
    state: &Arc<Mutex<McpBridgeState>>,
) -> Result<(), String> {
    let request = read_http_request(stream)?;
    if request.method != "POST" {
        write_json_response(
            stream,
            405,
            &McpBridgeErrorResponse {
                error: crate::mcp::models::McpBridgeErrorBody {
                    kind: "method_not_allowed".to_string(),
                    message: "Only POST is supported.".to_string(),
                    mcp: None,
                },
            },
        )?;
        return Ok(());
    }

    let Some(provided_token) = request.headers.get(token_header) else {
        write_json_response(
            stream,
            401,
            &McpBridgeErrorResponse {
                error: crate::mcp::models::McpBridgeErrorBody {
                    kind: "unauthorized".to_string(),
                    message: "Missing MCP bridge token.".to_string(),
                    mcp: None,
                },
            },
        )?;
        return Ok(());
    };
    if provided_token != token {
        write_json_response(
            stream,
            403,
            &McpBridgeErrorResponse {
                error: crate::mcp::models::McpBridgeErrorBody {
                    kind: "forbidden".to_string(),
                    message: "Invalid MCP bridge token.".to_string(),
                    mcp: None,
                },
            },
        )?;
        return Ok(());
    }

    let Some(tool_name) = request.path.strip_prefix("/mcp/tools/") else {
        write_json_response(
            stream,
            404,
            &McpBridgeErrorResponse {
                error: crate::mcp::models::McpBridgeErrorBody {
                    kind: "not_found".to_string(),
                    message: "Unknown MCP bridge route.".to_string(),
                    mcp: None,
                },
            },
        )?;
        return Ok(());
    };

    let invocation: McpBridgeInvocationRequest = serde_json::from_slice(&request.body)
        .map_err(|err| format!("Invalid MCP bridge request body: {err}"))?;
    let response = state
        .lock()
        .map_err(|_| "MCP bridge state lock poisoned.".to_string())?
        .invoke_tool(tool_name, invocation);

    match response {
        Ok(response) => write_json_response(stream, 200, &response)?,
        Err(err) => write_json_response(stream, err.status_code, &err.body)?,
    }

    Ok(())
}

struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut buffer = Vec::new();
    let mut header_end = None;
    let mut chunk = [0u8; 4096];

    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|err| format!("Failed to read MCP bridge HTTP request: {err}"))?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if header_end.is_none() {
            header_end = find_header_end(&buffer);
        }
        if let Some(end) = header_end {
            let headers_text = std::str::from_utf8(&buffer[..end])
                .map_err(|err| format!("MCP bridge request headers are not UTF-8: {err}"))?;
            let content_length = parse_content_length(headers_text)?;
            if buffer.len() >= end + content_length {
                let body = buffer[end..end + content_length].to_vec();
                return parse_http_request(headers_text, body);
            }
        }
    }

    Err("Incomplete MCP bridge HTTP request.".to_string())
}

fn parse_http_request(headers: &str, body: Vec<u8>) -> Result<HttpRequest, String> {
    let mut lines = headers.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| "Missing HTTP request line.".to_string())?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| "Missing HTTP method.".to_string())?
        .to_string();
    let path = request_parts
        .next()
        .ok_or_else(|| "Missing HTTP path.".to_string())?
        .to_string();

    let mut parsed_headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            parsed_headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    Ok(HttpRequest {
        method,
        path,
        headers: parsed_headers,
        body,
    })
}

fn write_json_response<T: serde::Serialize>(
    stream: &mut TcpStream,
    status_code: u16,
    body: &T,
) -> Result<(), String> {
    let payload = serde_json::to_string(body)
        .map_err(|err| format!("Failed to serialize MCP bridge response: {err}"))?;
    let status_text = match status_code {
        200 => "OK",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        504 => "Gateway Timeout",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status_code} {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    );
    stream
        .write_all(response.as_bytes())
        .and_then(|_| stream.flush())
        .map_err(|err| format!("Failed to write MCP bridge HTTP response: {err}"))
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn parse_content_length(headers: &str) -> Result<usize, String> {
    for line in headers.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("Content-Length") {
            return value
                .trim()
                .parse::<usize>()
                .map_err(|err| format!("Invalid Content-Length header: {err}"));
        }
    }

    Ok(0)
}
