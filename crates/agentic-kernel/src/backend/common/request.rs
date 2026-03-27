use anyhow::{Error as E, Result};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use super::response::{
    drain_chunked_body, find_header_end, is_timeout_error, parse_content_length,
    parse_http_headers, push_identity_body, timeout_error_message, HttpJsonResponse,
    HttpStreamControl, ParsedHttpHeaders,
};

pub(crate) struct HttpRequestOptions<'a> {
    pub(crate) timeout_ms: u64,
    pub(crate) max_request_bytes: usize,
    pub(crate) max_response_bytes: usize,
    pub(crate) extra_headers: Option<&'a HashMap<String, String>>,
}

#[derive(Debug, Clone)]
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

#[cfg(test)]
#[path = "tests/request.rs"]
mod tests;
