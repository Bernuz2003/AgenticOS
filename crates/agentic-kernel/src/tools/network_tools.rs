use std::env;
use std::fs;
use std::io::Read;
use std::time::Duration;

use agentic_kernel_macros::agentic_tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::kernel_config;

use super::error::ToolError;
use super::invocation::ToolContext;
use super::path_guard::resolve_safe_path_for_context;
use super::policy::{enforce_remote_host_policy, remote_http_max_response_bytes, syscall_config};

const DEFAULT_WEB_SEARCH_BASE_URL: &str = "https://api.duckduckgo.com/";
const DEFAULT_WEB_SEARCH_MAX_RESULTS: usize = 5;
const MAX_WEB_SEARCH_RESULTS: usize = 10;
const MAX_FETCH_LINKS: usize = 20;

#[derive(Debug, Clone)]
struct ParsedUrl {
    host: String,
    port: u16,
}

#[derive(Debug, Clone)]
struct FetchedResponse {
    status_code: u16,
    url: String,
    body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct HttpGetJsonInput {
    url: String,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
struct HttpGetJsonOutput {
    output: String,
    url: String,
    status_code: u16,
    json: serde_json::Value,
}

#[agentic_tool(
    name = "http_get_json",
    description = "Fetch a JSON document over HTTP(S) in read-only mode.",
    input_example = serde_json::json!({"url": "https://example.com/data.json", "timeout_ms": 5000}),
    capabilities = ["http", "json", "read"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn http_get_json(
    input: HttpGetJsonInput,
    _ctx: &ToolContext,
) -> Result<HttpGetJsonOutput, ToolError> {
    let response = fetch_url_bytes(
        "http_get_json",
        &input.url,
        input.timeout_ms,
        Some("application/json"),
    )?;
    let json: serde_json::Value = serde_json::from_slice(&response.body).map_err(|err| {
        ToolError::ExecutionFailed(
            "http_get_json".into(),
            format!("Response was not valid JSON: {err}"),
        )
    })?;

    Ok(HttpGetJsonOutput {
        output: format!(
            "Fetched JSON from '{}' (HTTP {}).",
            response.url, response.status_code
        ),
        url: response.url,
        status_code: response.status_code,
        json,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DownloadUrlInput {
    url: String,
    path: String,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct DownloadUrlOutput {
    output: String,
    url: String,
    path: String,
    status_code: u16,
    bytes_written: usize,
    created: bool,
}

#[agentic_tool(
    name = "download_url",
    description = "Download a remote resource and save it into the process-scoped workspace.",
    input_example = serde_json::json!({"url": "https://example.com/changelog.txt", "path": "downloads/changelog.txt", "timeout_ms": 5000}),
    capabilities = ["http", "download", "write"],
    dangerous = true,
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn download_url(
    input: DownloadUrlInput,
    ctx: &ToolContext,
) -> Result<DownloadUrlOutput, ToolError> {
    if input.path.trim().is_empty() {
        return Err(ToolError::InvalidInput(
            "download_url".into(),
            "field 'path' cannot be empty".into(),
        ));
    }

    let response = fetch_url_bytes("download_url", &input.url, input.timeout_ms, None)?;
    let path = resolve_safe_path_for_context(&input.path, ctx)
        .map_err(|err| ToolError::ExecutionFailed("download_url".into(), err))?;
    let created = !path.exists();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ToolError::ExecutionFailed(
                "download_url".into(),
                format!("Failed to create parent dir: {err}"),
            )
        })?;
    }

    fs::write(&path, &response.body).map_err(|err| {
        ToolError::ExecutionFailed("download_url".into(), format!("Write failed: {err}"))
    })?;

    Ok(DownloadUrlOutput {
        output: format!(
            "Downloaded '{}' to '{}' ({} bytes).",
            response.url,
            input.path,
            response.body.len()
        ),
        url: response.url,
        path: input.path,
        status_code: response.status_code,
        bytes_written: response.body.len(),
        created,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WebFetchInput {
    url: String,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct WebFetchOutput {
    output: String,
    url: String,
    status_code: u16,
    title: Option<String>,
    text: String,
    links: Vec<String>,
}

#[agentic_tool(
    name = "web_fetch",
    description = "Fetch a web page over HTTP(S) and extract readable text and links.",
    input_example = serde_json::json!({"url": "https://example.com/docs", "timeout_ms": 5000}),
    capabilities = ["web", "http", "read"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn web_fetch(input: WebFetchInput, _ctx: &ToolContext) -> Result<WebFetchOutput, ToolError> {
    let response = fetch_url_bytes("web_fetch", &input.url, input.timeout_ms, Some("text/html"))?;
    let html = String::from_utf8(response.body.clone()).map_err(|err| {
        ToolError::ExecutionFailed(
            "web_fetch".into(),
            format!("Response was not UTF-8 text: {err}"),
        )
    })?;
    let title = extract_html_title(&html);
    let text = normalize_extracted_text(&html_to_text(&html));
    let links = extract_links(&html, MAX_FETCH_LINKS);
    let summary = if let Some(title) = title.as_deref() {
        format!("Fetched '{}' from '{}'.", title, response.url)
    } else {
        format!("Fetched page from '{}'.", response.url)
    };

    Ok(WebFetchOutput {
        output: summary,
        url: response.url,
        status_code: response.status_code,
        title,
        text,
        links,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WebSearchInput {
    query: String,
    max_results: Option<u64>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct WebSearchResult {
    title: String,
    url: String,
    snippet: String,
    rank: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct WebSearchOutput {
    output: String,
    query: String,
    provider: String,
    results: Vec<WebSearchResult>,
    truncated: bool,
}

#[agentic_tool(
    name = "web_search",
    description = "Search the web through the configured JSON search provider and return ranked results.",
    input_example = serde_json::json!({"query": "agentic os rust macros", "max_results": 5, "timeout_ms": 5000}),
    capabilities = ["web", "search"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn web_search(input: WebSearchInput, _ctx: &ToolContext) -> Result<WebSearchOutput, ToolError> {
    let query = input.query.trim();
    if query.is_empty() {
        return Err(ToolError::InvalidInput(
            "web_search".into(),
            "field 'query' cannot be empty".into(),
        ));
    }

    let max_results = normalize_limit(
        input.max_results,
        DEFAULT_WEB_SEARCH_MAX_RESULTS,
        MAX_WEB_SEARCH_RESULTS,
    );
    let base_url = env::var("AGENTIC_WEB_SEARCH_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_WEB_SEARCH_BASE_URL.to_string());
    let separator = if base_url.contains('?') { '&' } else { '?' };
    let url = format!(
        "{}{}q={}&format=json&no_html=1&no_redirect=1",
        base_url,
        separator,
        percent_encode_component(query)
    );
    let response = fetch_url_bytes(
        "web_search",
        &url,
        input.timeout_ms,
        Some("application/json"),
    )?;
    let json: serde_json::Value = serde_json::from_slice(&response.body).map_err(|err| {
        ToolError::ExecutionFailed(
            "web_search".into(),
            format!("Search provider returned invalid JSON: {err}"),
        )
    })?;

    let mut results = Vec::new();
    collect_search_results(&json, &mut results);
    dedup_search_results(&mut results);
    let truncated = results.len() > max_results;
    results.truncate(max_results);

    let results = results
        .into_iter()
        .enumerate()
        .map(|(index, entry)| WebSearchResult {
            title: entry.title,
            url: entry.url,
            snippet: entry.snippet,
            rank: index as u64 + 1,
        })
        .collect::<Vec<_>>();

    let output = if results.is_empty() {
        format!("No search results for '{}'.", query)
    } else {
        format!(
            "Search results for '{}':\n{}",
            query,
            results
                .iter()
                .map(|entry| format!("{}. {} - {}", entry.rank, entry.title, entry.url))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };

    Ok(WebSearchOutput {
        output,
        query: query.to_string(),
        provider: search_provider_label(&base_url),
        results,
        truncated,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchCandidate {
    title: String,
    url: String,
    snippet: String,
}

fn fetch_url_bytes(
    tool_name: &str,
    url: &str,
    timeout_ms: Option<u64>,
    accept: Option<&str>,
) -> Result<FetchedResponse, ToolError> {
    let parsed =
        parse_network_url(url).map_err(|err| ToolError::InvalidInput(tool_name.into(), err))?;
    enforce_remote_host_policy(tool_name, &parsed.host, parsed.port)
        .map_err(|err| ToolError::PolicyDenied(tool_name.into(), err))?;

    let timeout_ms = timeout_ms.unwrap_or_else(default_timeout_ms).max(1);
    let mut request = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(timeout_ms))
        .timeout_read(Duration::from_millis(timeout_ms))
        .timeout_write(Duration::from_millis(timeout_ms))
        .redirects(5)
        .build()
        .get(url);
    if let Some(accept) = accept {
        request = request.set("Accept", accept);
    }

    let response = request
        .call()
        .map_err(|err| classify_ureq_error(tool_name, err, timeout_ms))?;
    let status_code = response.status();
    let final_url = response.get_url().to_string();
    let body = read_response_body(
        tool_name,
        response.into_reader(),
        remote_http_max_response_bytes(),
    )?;

    Ok(FetchedResponse {
        status_code,
        url: final_url,
        body,
    })
}

fn read_response_body(
    tool_name: &str,
    reader: impl Read,
    max_bytes: usize,
) -> Result<Vec<u8>, ToolError> {
    let mut limited = reader.take(max_bytes as u64 + 1);
    let mut body = Vec::new();
    limited.read_to_end(&mut body).map_err(|err| {
        ToolError::ExecutionFailed(tool_name.into(), format!("Read failed: {err}"))
    })?;
    if body.len() > max_bytes {
        return Err(ToolError::ExecutionFailed(
            tool_name.into(),
            format!(
                "Response exceeded limit ({} > {} bytes).",
                body.len(),
                max_bytes
            ),
        ));
    }
    Ok(body)
}

fn default_timeout_ms() -> u64 {
    syscall_config().timeout_s.max(1).saturating_mul(1_000)
}

fn classify_ureq_error(tool_name: &str, err: ureq::Error, timeout_ms: u64) -> ToolError {
    match err {
        ureq::Error::Status(status, response) => ToolError::ExecutionFailed(
            tool_name.into(),
            format!("HTTP {} ({}).", status, response.status_text()),
        ),
        ureq::Error::Transport(transport) => {
            let detail = transport.to_string();
            if detail.to_ascii_lowercase().contains("timed out") {
                ToolError::Timeout(tool_name.into(), timeout_ms)
            } else {
                ToolError::BackendUnavailable(tool_name.into(), detail)
            }
        }
    }
}

fn parse_network_url(url: &str) -> Result<ParsedUrl, String> {
    let (scheme, remainder) = if let Some(rest) = url.strip_prefix("http://") {
        ("http", rest)
    } else if let Some(rest) = url.strip_prefix("https://") {
        ("https", rest)
    } else {
        return Err("only http:// and https:// URLs are supported".into());
    };
    let authority = remainder.split('/').next().unwrap_or_default();
    let authority = authority.split('?').next().unwrap_or_default();
    if authority.is_empty() {
        return Err("URL host cannot be empty".into());
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port_text))
            if !host.is_empty() && port_text.chars().all(|ch| ch.is_ascii_digit()) =>
        {
            let port = port_text
                .parse::<u16>()
                .map_err(|_| format!("invalid port in URL '{url}'"))?;
            (host.to_string(), port)
        }
        _ => (
            authority.to_string(),
            if scheme == "https" { 443 } else { 80 },
        ),
    };

    Ok(ParsedUrl { host, port })
}

fn extract_html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title>")?;
    let end = lower[start + 7..].find("</title>")?;
    let raw = &html[start + 7..start + 7 + end];
    let title = normalize_extracted_text(&decode_html_entities(raw));
    (!title.is_empty()).then_some(title)
}

fn html_to_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    let mut tag_buffer = String::new();
    let mut last_was_whitespace = false;

    for ch in html.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
                let tag = tag_buffer.trim().to_ascii_lowercase();
                if tag.starts_with("br")
                    || tag.starts_with("/p")
                    || tag.starts_with("/div")
                    || tag.starts_with("/li")
                    || tag.starts_with("/h")
                {
                    text.push('\n');
                    last_was_whitespace = true;
                }
                tag_buffer.clear();
            } else {
                tag_buffer.push(ch);
            }
            continue;
        }

        if ch == '<' {
            in_tag = true;
            continue;
        }

        if ch.is_whitespace() {
            if !last_was_whitespace {
                text.push(' ');
                last_was_whitespace = true;
            }
        } else {
            text.push(ch);
            last_was_whitespace = false;
        }
    }

    decode_html_entities(&text)
}

fn extract_links(html: &str, limit: usize) -> Vec<String> {
    let mut links = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0;

    while links.len() < limit {
        let Some(offset) = lower[cursor..].find("href=") else {
            break;
        };
        cursor += offset + 5;
        let rest = &html[cursor..];
        let Some(quote) = rest.chars().next() else {
            break;
        };
        if quote != '"' && quote != '\'' {
            continue;
        }
        let value = &rest[1..];
        let Some(end) = value.find(quote) else {
            break;
        };
        let link = value[..end].trim();
        if !link.is_empty() && !links.iter().any(|existing| existing == link) {
            links.push(link.to_string());
        }
        cursor += end + 2;
    }

    links
}

fn decode_html_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn normalize_extracted_text(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_limit(raw: Option<u64>, default: usize, max: usize) -> usize {
    raw.unwrap_or(default as u64).clamp(1, max as u64) as usize
}

fn percent_encode_component(input: &str) -> String {
    let mut output = String::new();
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{:02X}", byte));
        }
    }
    output
}

fn collect_search_results(value: &serde_json::Value, output: &mut Vec<SearchCandidate>) {
    if let Some(abstract_url) = value.get("AbstractURL").and_then(serde_json::Value::as_str) {
        let abstract_text = value
            .get("AbstractText")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let heading = value
            .get("Heading")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(abstract_url);
        if !abstract_url.trim().is_empty() && !abstract_text.trim().is_empty() {
            output.push(SearchCandidate {
                title: heading.trim().to_string(),
                url: abstract_url.trim().to_string(),
                snippet: abstract_text.trim().to_string(),
            });
        }
    }

    if let Some(results) = value.get("Results").and_then(serde_json::Value::as_array) {
        for item in results {
            push_search_candidate(item, output);
        }
    }
    if let Some(related) = value
        .get("RelatedTopics")
        .and_then(serde_json::Value::as_array)
    {
        for item in related {
            if let Some(topics) = item.get("Topics").and_then(serde_json::Value::as_array) {
                for topic in topics {
                    push_search_candidate(topic, output);
                }
            } else {
                push_search_candidate(item, output);
            }
        }
    }
}

fn push_search_candidate(value: &serde_json::Value, output: &mut Vec<SearchCandidate>) {
    let Some(url) = value.get("FirstURL").and_then(serde_json::Value::as_str) else {
        return;
    };
    let Some(text) = value.get("Text").and_then(serde_json::Value::as_str) else {
        return;
    };
    let title = text.split(" - ").next().unwrap_or(text).trim();
    if title.is_empty() || url.trim().is_empty() {
        return;
    }

    output.push(SearchCandidate {
        title: title.to_string(),
        url: url.trim().to_string(),
        snippet: text.trim().to_string(),
    });
}

fn dedup_search_results(results: &mut Vec<SearchCandidate>) {
    let mut deduped = Vec::new();
    for result in results.drain(..) {
        if !deduped
            .iter()
            .any(|existing: &SearchCandidate| existing.url == result.url)
        {
            deduped.push(result);
        }
    }
    *results = deduped;
}

fn search_provider_label(base_url: &str) -> String {
    parse_network_url(base_url)
        .map(|parsed| parsed.host)
        .unwrap_or_else(|_| kernel_config().tools.remote_http_allowed_hosts.join(","))
}

#[cfg(test)]
#[path = "tests/network.rs"]
mod tests;
