use anyhow::{Error as E, Result};
use std::io::{self, ErrorKind};

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

#[derive(Debug, Clone)]
pub(crate) struct ParsedHttpHeaders {
    pub(crate) status_code: u16,
    pub(crate) status_line: String,
    pub(crate) content_length: Option<usize>,
    pub(crate) chunked: bool,
}

pub(crate) fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

pub(crate) fn parse_http_headers(headers: &str) -> Result<ParsedHttpHeaders> {
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

pub(crate) fn parse_content_length(headers: &str) -> Result<Option<usize>> {
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

pub(crate) fn push_identity_body<F>(
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

pub(crate) fn drain_chunked_body<F>(
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

pub(crate) fn is_timeout_error(err: &io::Error) -> bool {
    matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
        || matches!(err.raw_os_error(), Some(11 | 35 | 60 | 110 | 10035 | 10060))
}

pub(crate) fn timeout_error_message(
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
