use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use agentic_kernel_macros::agentic_tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::error::ToolError;
use super::invocation::ToolContext;
use super::path_guard::{display_path, resolve_safe_path_for_context};
use super::workspace_tools::ensure_non_empty_path;

const MAX_STRUCTURED_PARSE_BYTES: u64 = 512 * 1024;
const MAX_PREVIEW_BYTES: usize = 16 * 1024;
const MAX_PREVIEW_CHARS: usize = 3_000;
const MAX_JSON_KEYS: usize = 12;
const MAX_CSV_PREVIEW_ROWS: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct InspectDocumentInput {
    path: String,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DocumentKind {
    Text,
    Json,
    Csv,
    Pdf,
    Image,
    Binary,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct TextDocumentPreview {
    preview_line_count: usize,
    truncated: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct JsonDocumentPreview {
    top_level_kind: String,
    object_keys: Vec<String>,
    item_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct CsvDocumentPreview {
    columns: Vec<String>,
    preview_rows: Vec<Vec<String>>,
    preview_row_count: usize,
    truncated: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
struct InspectDocumentOutput {
    output: String,
    path: String,
    detected_kind: DocumentKind,
    mime_type: Option<String>,
    bytes: u64,
    sha256: String,
    preview: Option<String>,
    text_preview: Option<TextDocumentPreview>,
    json_preview: Option<JsonDocumentPreview>,
    csv_preview: Option<CsvDocumentPreview>,
}

#[agentic_tool(
    name = "inspect_document",
    description = "Inspect a document or binary file inside the process-scoped workspace and return metadata, hash, kind detection and structured previews for text, JSON and CSV when possible.",
    input_example = serde_json::json!({"path": "docs/AGENTICOS_VISION_ROADMAP_v1.1.0.md"}),
    capabilities = ["fs", "read", "document"],
    allowed_callers = [AgentText, AgentSupervisor, Programmatic]
)]
fn inspect_document(
    input: InspectDocumentInput,
    ctx: &ToolContext,
) -> Result<InspectDocumentOutput, ToolError> {
    ensure_non_empty_path("inspect_document", &input.path)?;
    let path = resolve_safe_path_for_context(&input.path, ctx)
        .map_err(|err| ToolError::ExecutionFailed("inspect_document".into(), err))?;
    let metadata = fs::metadata(&path).map_err(|err| {
        ToolError::ExecutionFailed(
            "inspect_document".into(),
            format!("Metadata lookup failed: {err}"),
        )
    })?;

    if !metadata.is_file() {
        return Err(ToolError::InvalidInput(
            "inspect_document".into(),
            format!("'{}' is not a regular file", input.path),
        ));
    }

    let bytes = metadata.len();
    let sha256 = sha256_file(&path).map_err(|err| {
        ToolError::ExecutionFailed("inspect_document".into(), format!("Hashing failed: {err}"))
    })?;
    let preview_bytes = read_prefix_bytes(&path, MAX_PREVIEW_BYTES).map_err(|err| {
        ToolError::ExecutionFailed("inspect_document".into(), format!("Read failed: {err}"))
    })?;
    let full_bytes = if bytes <= MAX_STRUCTURED_PARSE_BYTES {
        Some(fs::read(&path).map_err(|err| {
            ToolError::ExecutionFailed("inspect_document".into(), format!("Read failed: {err}"))
        })?)
    } else {
        None
    };
    let display = display_path(&path).map_err(|err| {
        ToolError::ExecutionFailed(
            "inspect_document".into(),
            format!("Display path failed: {err}"),
        )
    })?;

    let (detected_kind, mime_type) = detect_document_kind(&path, &preview_bytes);
    let mut preview = None;
    let mut text_preview = None;
    let mut json_preview = None;
    let mut csv_preview = None;

    match detected_kind {
        DocumentKind::Text => {
            let source_bytes = full_bytes.as_deref().unwrap_or(&preview_bytes);
            let (text, was_lossy) = decode_text_preview(source_bytes);
            let truncated =
                was_lossy || full_bytes.is_none() || text.chars().count() > MAX_PREVIEW_CHARS;
            let preview_text = truncate_preview_chars(&text);
            let preview_line_count = preview_text.lines().count();
            preview = Some(preview_text.clone());
            text_preview = Some(TextDocumentPreview {
                preview_line_count,
                truncated,
            });
        }
        DocumentKind::Json => {
            let source_bytes = full_bytes.as_deref().unwrap_or(&preview_bytes);
            let (text, was_lossy) = decode_text_preview(source_bytes);
            let preview_text = truncate_preview_chars(&text);
            preview = Some(preview_text);

            if !was_lossy {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    json_preview = Some(build_json_preview(&value));
                }
            }
        }
        DocumentKind::Csv => {
            let source_bytes = full_bytes.as_deref().unwrap_or(&preview_bytes);
            let (text, _) = decode_text_preview(source_bytes);
            let preview_text = truncate_preview_chars(&text);
            preview = Some(preview_text);
            csv_preview = build_csv_preview(&text);
        }
        DocumentKind::Pdf | DocumentKind::Image | DocumentKind::Binary => {}
    }

    let output = build_summary(
        &display,
        detected_kind,
        mime_type.as_deref(),
        bytes,
        &sha256,
        json_preview.as_ref(),
        csv_preview.as_ref(),
        text_preview.as_ref(),
    );

    Ok(InspectDocumentOutput {
        output,
        path: display,
        detected_kind,
        mime_type,
        bytes,
        sha256,
        preview,
        text_preview,
        json_preview,
        csv_preview,
    })
}

fn build_summary(
    path: &str,
    kind: DocumentKind,
    mime_type: Option<&str>,
    bytes: u64,
    sha256: &str,
    json_preview: Option<&JsonDocumentPreview>,
    csv_preview: Option<&CsvDocumentPreview>,
    text_preview: Option<&TextDocumentPreview>,
) -> String {
    let sha_prefix = &sha256[..sha256.len().min(12)];
    let mime_suffix = mime_type
        .map(|value| format!(" · {value}"))
        .unwrap_or_default();
    let detail = match kind {
        DocumentKind::Json => json_preview
            .map(|preview| match preview.top_level_kind.as_str() {
                "object" if !preview.object_keys.is_empty() => {
                    format!("object keys: {}", preview.object_keys.join(", "))
                }
                "array" => format!("array items: {}", preview.item_count.unwrap_or_default()),
                other => format!("top-level: {other}"),
            })
            .unwrap_or_else(|| "structured preview unavailable".to_string()),
        DocumentKind::Csv => csv_preview
            .map(|preview| {
                if preview.columns.is_empty() {
                    format!("preview rows: {}", preview.preview_row_count)
                } else {
                    format!(
                        "columns: {} · preview rows: {}",
                        preview.columns.join(", "),
                        preview.preview_row_count
                    )
                }
            })
            .unwrap_or_else(|| "structured preview unavailable".to_string()),
        DocumentKind::Text => text_preview
            .map(|preview| format!("preview lines: {}", preview.preview_line_count))
            .unwrap_or_else(|| "text preview unavailable".to_string()),
        DocumentKind::Pdf => "metadata only".to_string(),
        DocumentKind::Image => "metadata only".to_string(),
        DocumentKind::Binary => "metadata only".to_string(),
    };

    format!(
        "{} · {:?}{} · {} bytes · sha256 {} · {}",
        path, kind, mime_suffix, bytes, sha_prefix, detail
    )
}

fn detect_document_kind(path: &Path, preview_bytes: &[u8]) -> (DocumentKind, Option<String>) {
    if preview_bytes.starts_with(b"%PDF-") {
        return (DocumentKind::Pdf, Some("application/pdf".to_string()));
    }
    if preview_bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return (DocumentKind::Image, Some("image/png".to_string()));
    }
    if preview_bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return (DocumentKind::Image, Some("image/jpeg".to_string()));
    }
    if preview_bytes.starts_with(b"GIF87a") || preview_bytes.starts_with(b"GIF89a") {
        return (DocumentKind::Image, Some("image/gif".to_string()));
    }
    if preview_bytes.len() >= 12
        && &preview_bytes[0..4] == b"RIFF"
        && &preview_bytes[8..12] == b"WEBP"
    {
        return (DocumentKind::Image, Some("image/webp".to_string()));
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());

    match extension.as_deref() {
        Some("json") => return (DocumentKind::Json, Some("application/json".to_string())),
        Some("csv") => return (DocumentKind::Csv, Some("text/csv".to_string())),
        Some("tsv") => {
            return (
                DocumentKind::Csv,
                Some("text/tab-separated-values".to_string()),
            )
        }
        Some("md") => return (DocumentKind::Text, Some("text/markdown".to_string())),
        Some("txt") | Some("log") | Some("rs") | Some("ts") | Some("tsx") | Some("js")
        | Some("jsx") | Some("py") | Some("sh") | Some("toml") | Some("yaml") | Some("yml")
        | Some("html") | Some("css") => {
            return (DocumentKind::Text, Some("text/plain".to_string()))
        }
        _ => {}
    }

    if std::str::from_utf8(preview_bytes).is_ok() {
        return (DocumentKind::Text, Some("text/plain".to_string()));
    }

    (DocumentKind::Binary, None)
}

fn build_json_preview(value: &serde_json::Value) -> JsonDocumentPreview {
    match value {
        serde_json::Value::Object(map) => JsonDocumentPreview {
            top_level_kind: "object".to_string(),
            object_keys: map.keys().take(MAX_JSON_KEYS).cloned().collect(),
            item_count: Some(map.len()),
        },
        serde_json::Value::Array(items) => JsonDocumentPreview {
            top_level_kind: "array".to_string(),
            object_keys: Vec::new(),
            item_count: Some(items.len()),
        },
        serde_json::Value::String(_) => JsonDocumentPreview {
            top_level_kind: "string".to_string(),
            object_keys: Vec::new(),
            item_count: None,
        },
        serde_json::Value::Number(_) => JsonDocumentPreview {
            top_level_kind: "number".to_string(),
            object_keys: Vec::new(),
            item_count: None,
        },
        serde_json::Value::Bool(_) => JsonDocumentPreview {
            top_level_kind: "boolean".to_string(),
            object_keys: Vec::new(),
            item_count: None,
        },
        serde_json::Value::Null => JsonDocumentPreview {
            top_level_kind: "null".to_string(),
            object_keys: Vec::new(),
            item_count: None,
        },
    }
}

fn build_csv_preview(content: &str) -> Option<CsvDocumentPreview> {
    let lines: Vec<&str> = content
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }

    let delimiter = if lines[0].contains('\t') { '\t' } else { ',' };
    let columns = split_delimited_line(lines[0], delimiter);
    let data_lines = lines.iter().skip(1).copied().collect::<Vec<_>>();
    let preview_rows = data_lines
        .iter()
        .take(MAX_CSV_PREVIEW_ROWS)
        .map(|line| split_delimited_line(line, delimiter))
        .collect::<Vec<_>>();

    Some(CsvDocumentPreview {
        columns,
        preview_row_count: preview_rows.len(),
        preview_rows,
        truncated: data_lines.len() > MAX_CSV_PREVIEW_ROWS,
    })
}

fn split_delimited_line(line: &str, delimiter: char) -> Vec<String> {
    line.split(delimiter)
        .map(|cell| cell.trim().trim_matches('"').to_string())
        .collect()
}

fn truncate_preview_chars(text: &str) -> String {
    let mut preview = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= MAX_PREVIEW_CHARS {
            preview.push_str("\n... [preview truncated]");
            break;
        }
        preview.push(ch);
    }
    preview
}

fn decode_text_preview(bytes: &[u8]) -> (String, bool) {
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_string(), false),
        Err(_) => (String::from_utf8_lossy(bytes).into_owned(), true),
    }
}

fn read_prefix_bytes(path: &Path, max_bytes: usize) -> Result<Vec<u8>, std::io::Error> {
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0_u8; max_bytes];
    let read = file.read(&mut buffer)?;
    buffer.truncate(read);
    Ok(buffer)
}

fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let mut file = fs::File::open(path)?;
    file.seek(SeekFrom::Start(0))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
#[path = "tests/document.rs"]
mod tests;
