use anyhow::{Error as E, Result};
use std::collections::HashMap;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Debug, Clone)]
pub(crate) struct HttpJsonResponse {
    pub(crate) status_code: u16,
    pub(crate) status_line: String,
    pub(crate) body: String,
    pub(crate) json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HttpStreamControl {
    Continue,
    Stop,
}

pub(crate) struct HttpRequestOptions<'a> {
    pub(crate) timeout_ms: u64,
    pub(crate) max_request_bytes: usize,
    pub(crate) max_response_bytes: usize,
    pub(crate) extra_headers: Option<&'a HashMap<String, String>>,
}

#[derive(Clone)]
pub(crate) struct HttpEndpoint {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) base_path: String,
}

impl HttpEndpoint {
    pub(crate) fn parse(url: &str) -> Result<Self> {
        let stripped = url.strip_prefix("http://").ok_or_else(|| {
            E::msg("Only http:// endpoints are currently supported for external llama.cpp RPC.")
        })?;

        let (host_port, path) = match stripped.split_once('/') {
            Some((host_port, rest)) => (host_port, format!("/{}", rest.trim_start_matches('/'))),
            None => (stripped, String::new()),
        };

        let (host, port) = match host_port.split_once(':') {
            Some((host, port_str)) => {
                let port = port_str
                    .parse::<u16>()
                    .map_err(|_| E::msg(format!("Invalid port in external endpoint '{}'.", url)))?;
                (host.to_string(), port)
            }
            None => (host_port.to_string(), 80),
        };

        if host.is_empty() {
            return Err(E::msg(format!(
                "Invalid external endpoint '{}': host is empty.",
                url
            )));
        }

        Ok(Self {
            host,
            port,
            base_path: path.trim_end_matches('/').to_string(),
        })
    }

    pub(crate) fn joined_path(&self, suffix: &str) -> String {
        format!("{}{suffix}", self.base_path)
    }

    pub(crate) fn request_json(
        &self,
        method: &str,
        path: &str,
        payload: Option<&serde_json::Value>,
        timeout_ms: u64,
    ) -> Result<HttpJsonResponse> {
        self.request_json_with_options(
            method,
            path,
            payload,
            HttpRequestOptions {
                timeout_ms,
                max_request_bytes: usize::MAX,
                max_response_bytes: usize::MAX,
                extra_headers: None,
            },
        )
    }

    pub(crate) fn request_json_with_options(
        &self,
        method: &str,
        path: &str,
        payload: Option<&serde_json::Value>,
        options: HttpRequestOptions<'_>,
    ) -> Result<HttpJsonResponse> {
        let addr = format!("{}:{}", self.host, self.port);
        let mut addrs = addr.to_socket_addrs().map_err(|e| {
            E::msg(format!(
                "Failed to resolve external RPC endpoint '{}': {}",
                addr, e
            ))
        })?;
        let socket_addr = addrs.next().ok_or_else(|| {
            E::msg(format!(
                "No address resolved for external RPC endpoint '{}'.",
                addr
            ))
        })?;
        let timeout = Duration::from_millis(options.timeout_ms);
        let mut stream = TcpStream::connect_timeout(&socket_addr, timeout).map_err(|e| {
            E::msg(format!(
                "Failed to connect to external RPC endpoint '{}': {}",
                addr, e
            ))
        })?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| E::msg(format!("Failed to configure read timeout: {}", e)))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| E::msg(format!("Failed to configure write timeout: {}", e)))?;

        let request_body = payload.map(|value| value.to_string()).unwrap_or_default();
        if request_body.len() > options.max_request_bytes {
            return Err(E::msg(format!(
                "External RPC request body exceeded limit ({} > {} bytes).",
                request_body.len(),
                options.max_request_bytes
            )));
        }
        let content_header = if payload.is_some() {
            format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                request_body.len()
            )
        } else {
            String::new()
        };
        let mut extra_header_block = String::new();
        if let Some(headers) = options.extra_headers {
            for (name, value) in headers {
                extra_header_block.push_str(name);
                extra_header_block.push_str(": ");
                extra_header_block.push_str(value);
                extra_header_block.push_str("\r\n");
            }
        }
        let request = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\n{}{}Connection: close\r\n\r\n{}",
            method, path, self.host, content_header, extra_header_block, request_body
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| E::msg(format!("Failed to write external RPC request: {}", e)))?;

        let mut response = Vec::new();
        let mut header_end: Option<usize> = None;
        let mut expected_total_bytes: Option<usize> = None;
        let mut chunk = [0u8; 4096];
        loop {
            let read = match stream.read(&mut chunk) {
                Ok(read) => read,
                Err(err) if is_timeout_error(&err) => {
                    return Err(E::msg(timeout_error_message(
                        &err,
                        options.timeout_ms,
                        response.is_empty(),
                        header_end.is_some(),
                    )));
                }
                Err(err) => {
                    return Err(E::msg(format!(
                        "Failed to read external RPC response: {}",
                        err
                    )));
                }
            };
            if read == 0 {
                break;
            }
            response.extend_from_slice(&chunk[..read]);
            if response.len() > options.max_response_bytes {
                return Err(E::msg(format!(
                    "External RPC response exceeded limit ({} > {} bytes).",
                    response.len(),
                    options.max_response_bytes
                )));
            }
            if header_end.is_none() {
                if let Some(parsed_header_end) = find_header_end(&response) {
                    let headers =
                        std::str::from_utf8(&response[..parsed_header_end]).map_err(|e| {
                            E::msg(format!("External RPC returned non-UTF8 headers: {}", e))
                        })?;
                    header_end = Some(parsed_header_end);
                    expected_total_bytes = parse_content_length(headers)?
                        .map(|content_length| parsed_header_end + content_length);
                }
            }
            if let Some(total_bytes) = expected_total_bytes {
                if response.len() >= total_bytes {
                    response.truncate(total_bytes);
                    break;
                }
            }
        }

        let response = String::from_utf8(response)
            .map_err(|e| E::msg(format!("External RPC returned non-UTF8 response: {}", e)))?;
        let (headers, body) = response
            .split_once("\r\n\r\n")
            .ok_or_else(|| E::msg("Malformed HTTP response from external RPC endpoint."))?;
        let status_line = headers.lines().next().unwrap_or_default();
        let status_code = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or_else(|| E::msg(format!("Malformed HTTP status line '{}'.", status_line)))?;

        Ok(HttpJsonResponse {
            status_code,
            status_line: status_line.to_string(),
            body: body.to_string(),
            json: serde_json::from_str(body).ok(),
        })
    }

    pub(crate) fn request_stream_with_options<F>(
        &self,
        method: &str,
        path: &str,
        payload: Option<&serde_json::Value>,
        options: HttpRequestOptions<'_>,
        mut on_body_chunk: F,
    ) -> Result<HttpJsonResponse>
    where
        F: FnMut(&[u8]) -> Result<HttpStreamControl>,
    {
        let addr = format!("{}:{}", self.host, self.port);
        let mut addrs = addr.to_socket_addrs().map_err(|e| {
            E::msg(format!(
                "Failed to resolve external RPC endpoint '{}': {}",
                addr, e
            ))
        })?;
        let socket_addr = addrs.next().ok_or_else(|| {
            E::msg(format!(
                "No address resolved for external RPC endpoint '{}'.",
                addr
            ))
        })?;
        let timeout = Duration::from_millis(options.timeout_ms);
        let mut stream = TcpStream::connect_timeout(&socket_addr, timeout).map_err(|e| {
            E::msg(format!(
                "Failed to connect to external RPC endpoint '{}': {}",
                addr, e
            ))
        })?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| E::msg(format!("Failed to configure read timeout: {}", e)))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| E::msg(format!("Failed to configure write timeout: {}", e)))?;

        let request_body = payload.map(|value| value.to_string()).unwrap_or_default();
        if request_body.len() > options.max_request_bytes {
            return Err(E::msg(format!(
                "External RPC request body exceeded limit ({} > {} bytes).",
                request_body.len(),
                options.max_request_bytes
            )));
        }
        let content_header = if payload.is_some() {
            format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                request_body.len()
            )
        } else {
            String::new()
        };
        let mut extra_header_block = String::new();
        if let Some(headers) = options.extra_headers {
            for (name, value) in headers {
                extra_header_block.push_str(name);
                extra_header_block.push_str(": ");
                extra_header_block.push_str(value);
                extra_header_block.push_str("\r\n");
            }
        }
        let request = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\n{}{}Connection: close\r\n\r\n{}",
            method, path, self.host, content_header, extra_header_block, request_body
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| E::msg(format!("Failed to write external RPC request: {}", e)))?;

        let mut response = Vec::new();
        let mut body = Vec::new();
        let mut header_end: Option<usize> = None;
        let mut parsed_headers: Option<ParsedHttpHeaders> = None;
        let mut chunk_buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        let mut stop_requested = false;

        loop {
            let read = match stream.read(&mut chunk) {
                Ok(read) => read,
                Err(err) if is_timeout_error(&err) => {
                    return Err(E::msg(timeout_error_message(
                        &err,
                        options.timeout_ms,
                        response.is_empty(),
                        header_end.is_some(),
                    )));
                }
                Err(err) => {
                    return Err(E::msg(format!(
                        "Failed to read external RPC response: {}",
                        err
                    )));
                }
            };
            if read == 0 {
                break;
            }

            response.extend_from_slice(&chunk[..read]);
            if response.len() > options.max_response_bytes {
                return Err(E::msg(format!(
                    "External RPC response exceeded limit ({} > {} bytes).",
                    response.len(),
                    options.max_response_bytes
                )));
            }

            if header_end.is_none() {
                if let Some(parsed_header_end) = find_header_end(&response) {
                    let headers =
                        std::str::from_utf8(&response[..parsed_header_end]).map_err(|e| {
                            E::msg(format!("External RPC returned non-UTF8 headers: {}", e))
                        })?;
                    header_end = Some(parsed_header_end);
                    parsed_headers = Some(parse_http_headers(headers)?);
                    let initial_body = response[parsed_header_end..].to_vec();
                    if !initial_body.is_empty() {
                        let control = if parsed_headers
                            .as_ref()
                            .is_some_and(|headers| headers.chunked)
                        {
                            chunk_buffer.extend_from_slice(&initial_body);
                            drain_chunked_body(&mut chunk_buffer, &mut body, &mut on_body_chunk)?
                        } else {
                            push_identity_body(&mut body, &initial_body, &mut on_body_chunk)?
                        };
                        if matches!(control, HttpStreamControl::Stop) {
                            stop_requested = true;
                            break;
                        }
                    }
                }
            } else if parsed_headers
                .as_ref()
                .is_some_and(|headers| headers.chunked)
            {
                chunk_buffer.extend_from_slice(&chunk[..read]);
                if matches!(
                    drain_chunked_body(&mut chunk_buffer, &mut body, &mut on_body_chunk)?,
                    HttpStreamControl::Stop
                ) {
                    stop_requested = true;
                    break;
                }
                if parsed_headers.as_ref().is_some_and(|headers| {
                    headers
                        .content_length
                        .is_some_and(|length| body.len() >= length)
                }) {
                    break;
                }
            } else if matches!(
                push_identity_body(&mut body, &chunk[..read], &mut on_body_chunk)?,
                HttpStreamControl::Stop
            ) {
                stop_requested = true;
                break;
            }

            if let Some(headers) = parsed_headers.as_ref() {
                if !headers.chunked
                    && headers
                        .content_length
                        .is_some_and(|content_length| body.len() >= content_length)
                {
                    break;
                }
            }
        }

        let Some(parsed_headers) = parsed_headers else {
            return Err(E::msg(
                "Malformed HTTP response from external RPC endpoint.",
            ));
        };
        let body = if let Some(content_length) = parsed_headers.content_length {
            &body[..body.len().min(content_length)]
        } else {
            body.as_slice()
        };
        let body = String::from_utf8(body.to_vec())
            .map_err(|e| E::msg(format!("External RPC returned non-UTF8 response: {}", e)))?;

        Ok(HttpJsonResponse {
            status_code: parsed_headers.status_code,
            status_line: parsed_headers.status_line,
            body: body.clone(),
            json: (!stop_requested)
                .then(|| serde_json::from_str(&body).ok())
                .flatten(),
        })
    }
}

#[derive(Debug, Clone)]
struct ParsedHttpHeaders {
    status_code: u16,
    status_line: String,
    content_length: Option<usize>,
    chunked: bool,
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn parse_http_headers(headers: &str) -> Result<ParsedHttpHeaders> {
    let status_line = headers.lines().next().unwrap_or_default().to_string();
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| E::msg(format!("Malformed HTTP status line '{}'.", status_line)))?;

    Ok(ParsedHttpHeaders {
        content_length: parse_content_length(headers)?,
        chunked: headers.lines().skip(1).any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.eq_ignore_ascii_case("Transfer-Encoding")
                    && value.to_ascii_lowercase().contains("chunked")
            })
        }),
        status_code,
        status_line,
    })
}

fn parse_content_length(headers: &str) -> Result<Option<usize>> {
    for line in headers.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("Content-Length") {
            let content_length = value.trim().parse::<usize>().map_err(|_| {
                E::msg(format!(
                    "Malformed HTTP response from external RPC endpoint: invalid Content-Length '{}'.",
                    value.trim()
                ))
            })?;
            return Ok(Some(content_length));
        }
    }

    Ok(None)
}

fn push_identity_body<F>(
    body: &mut Vec<u8>,
    bytes: &[u8],
    on_body_chunk: &mut F,
) -> Result<HttpStreamControl>
where
    F: FnMut(&[u8]) -> Result<HttpStreamControl>,
{
    if bytes.is_empty() {
        return Ok(HttpStreamControl::Continue);
    }
    body.extend_from_slice(bytes);
    on_body_chunk(bytes)
}

fn drain_chunked_body<F>(
    buffer: &mut Vec<u8>,
    body: &mut Vec<u8>,
    on_body_chunk: &mut F,
) -> Result<HttpStreamControl>
where
    F: FnMut(&[u8]) -> Result<HttpStreamControl>,
{
    loop {
        let Some(line_end) = buffer.windows(2).position(|window| window == b"\r\n") else {
            return Ok(HttpStreamControl::Continue);
        };
        let size_line = std::str::from_utf8(&buffer[..line_end]).map_err(|err| {
            E::msg(format!(
                "Malformed chunked HTTP response from external RPC endpoint: {}",
                err
            ))
        })?;
        let size_hex = size_line.split(';').next().unwrap_or_default().trim();
        let chunk_size = usize::from_str_radix(size_hex, 16).map_err(|_| {
            E::msg(format!(
                "Malformed chunked HTTP response from external RPC endpoint: invalid chunk size '{}'.",
                size_hex
            ))
        })?;
        let data_start = line_end + 2;
        if chunk_size == 0 {
            if buffer.len() < data_start + 2 {
                return Ok(HttpStreamControl::Continue);
            }
            buffer.drain(..data_start + 2);
            return Ok(HttpStreamControl::Continue);
        }
        let required = data_start + chunk_size + 2;
        if buffer.len() < required {
            return Ok(HttpStreamControl::Continue);
        }
        if &buffer[data_start + chunk_size..required] != b"\r\n" {
            return Err(E::msg(
                "Malformed chunked HTTP response from external RPC endpoint: missing chunk terminator.",
            ));
        }
        let chunk_bytes = buffer[data_start..data_start + chunk_size].to_vec();
        body.extend_from_slice(&chunk_bytes);
        buffer.drain(..required);
        if matches!(on_body_chunk(&chunk_bytes)?, HttpStreamControl::Stop) {
            return Ok(HttpStreamControl::Stop);
        }
    }
}

fn is_timeout_error(err: &io::Error) -> bool {
    matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
        || matches!(err.raw_os_error(), Some(11 | 35 | 60 | 110 | 10035 | 10060))
}

fn timeout_error_message(
    err: &io::Error,
    timeout_ms: u64,
    waiting_for_first_byte: bool,
    headers_received: bool,
) -> String {
    if waiting_for_first_byte {
        format!(
            "External RPC read timed out after {} ms while waiting for the first response byte ({}). The external backend is likely still computing the first token; increase [external_llamacpp].timeout_ms for slow local inference.",
            timeout_ms, err
        )
    } else if headers_received {
        format!(
            "External RPC read timed out after {} ms while waiting for the remaining response bytes ({}).",
            timeout_ms, err
        )
    } else {
        format!(
            "External RPC read timed out after {} ms while waiting for complete response headers ({}).",
            timeout_ms, err
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{is_timeout_error, HttpEndpoint, HttpRequestOptions};
    use std::io::{Error, ErrorKind, Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn recognizes_common_socket_timeout_errors() {
        assert!(is_timeout_error(&Error::from(ErrorKind::WouldBlock)));
        assert!(is_timeout_error(&Error::from(ErrorKind::TimedOut)));
        assert!(is_timeout_error(&Error::from_raw_os_error(11)));
        assert!(is_timeout_error(&Error::from_raw_os_error(110)));
    }

    #[test]
    fn request_json_stops_after_content_length_without_waiting_for_close() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind http listener");
        let addr = listener.local_addr().expect("listener addr");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept http client");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read request");
            let body = r#"{"ok":true}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response body");
            thread::sleep(Duration::from_millis(200));
        });

        let endpoint = HttpEndpoint::parse(&format!("http://{}", addr)).expect("parse endpoint");
        let response = endpoint
            .request_json_with_options(
                "GET",
                "/health",
                None,
                HttpRequestOptions {
                    timeout_ms: 50,
                    max_request_bytes: usize::MAX,
                    max_response_bytes: usize::MAX,
                    extra_headers: None,
                },
            )
            .expect("http client should not wait for socket close");

        assert_eq!(response.status_code, 200);
        assert_eq!(response.body, "{\"ok\":true}");

        server.join().expect("join http server");
    }
}
