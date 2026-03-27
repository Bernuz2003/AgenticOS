use crate::models::kernel::{TimelineItem, TimelineItemKind};

use super::markers::{find_next_marker, MarkerKind};

pub(crate) fn parse_stream_segments(
    item_prefix: &str,
    stream: &str,
    running: bool,
) -> Vec<TimelineItem> {
    let mut items = Vec::new();
    let mut cursor = 0usize;
    let mut item_index = 1usize;

    while cursor < stream.len() {
        let remaining = &stream[cursor..];
        let next_marker = find_next_marker(remaining);

        match next_marker {
            None => {
                push_timeline_text_item(
                    &mut items,
                    item_prefix,
                    &mut item_index,
                    TimelineItemKind::AssistantMessage,
                    remaining,
                    if running { "streaming" } else { "complete" },
                );
                break;
            }
            Some((offset, MarkerKind::Think)) => {
                if offset > 0 {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::AssistantMessage,
                        &remaining[..offset],
                        "complete",
                    );
                }

                let think_start = cursor + offset + "<think>".len();
                let think_rest = &stream[think_start..];
                if let Some(end_offset) = think_rest.find("</think>") {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::Thinking,
                        &think_rest[..end_offset],
                        "complete",
                    );
                    cursor = think_start + end_offset + "</think>".len();
                } else {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::Thinking,
                        think_rest,
                        if running { "streaming" } else { "complete" },
                    );
                    break;
                }
            }
            Some((offset, MarkerKind::BracketTool)) => {
                if offset > 0 {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::AssistantMessage,
                        &remaining[..offset],
                        "complete",
                    );
                }

                let tool_start = cursor + offset + 2;
                let tool_rest = &stream[tool_start..];
                if let Some(end_offset) = tool_rest.find("]]") {
                    let invocation = &tool_rest[..end_offset];
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        timeline_item_kind_for_invocation(invocation),
                        invocation,
                        "complete",
                    );
                    cursor = tool_start + end_offset + 2;
                } else {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::ToolCall,
                        tool_rest,
                        if running { "streaming" } else { "complete" },
                    );
                    break;
                }
            }
            Some((offset, MarkerKind::BareTool)) => {
                if offset > 0 {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::AssistantMessage,
                        &remaining[..offset],
                        "complete",
                    );
                }

                let tool_start = cursor + offset;
                let tool_rest = &stream[tool_start..];
                if tool_rest.starts_with("TOOL:") || tool_rest.starts_with("ACTION:") {
                    match extract_canonical_invocation_segment(tool_rest) {
                        CanonicalInvocationSegment::Parsed { raw, consumed } => {
                            push_timeline_text_item(
                                &mut items,
                                item_prefix,
                                &mut item_index,
                                timeline_item_kind_for_invocation(raw),
                                raw,
                                "complete",
                            );
                            cursor = tool_start + consumed;
                        }
                        CanonicalInvocationSegment::Incomplete => {
                            push_timeline_text_item(
                                &mut items,
                                item_prefix,
                                &mut item_index,
                                timeline_item_kind_for_invocation(tool_rest),
                                tool_rest,
                                if running { "streaming" } else { "complete" },
                            );
                            break;
                        }
                        CanonicalInvocationSegment::Invalid => {
                            if let Some(line_end_offset) = tool_rest.find('\n') {
                                let invocation = &tool_rest[..line_end_offset];
                                push_timeline_text_item(
                                    &mut items,
                                    item_prefix,
                                    &mut item_index,
                                    timeline_item_kind_for_invocation(invocation),
                                    invocation,
                                    "complete",
                                );
                                cursor = tool_start + line_end_offset + 1;
                            } else {
                                push_timeline_text_item(
                                    &mut items,
                                    item_prefix,
                                    &mut item_index,
                                    timeline_item_kind_for_invocation(tool_rest),
                                    tool_rest,
                                    if running { "streaming" } else { "complete" },
                                );
                                break;
                            }
                        }
                    }
                } else if let Some(line_end_offset) = tool_rest.find('\n') {
                    let invocation = &tool_rest[..line_end_offset];
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        timeline_item_kind_for_invocation(invocation),
                        invocation,
                        "complete",
                    );
                    cursor = tool_start + line_end_offset + 1;
                } else {
                    push_timeline_text_item(
                        &mut items,
                        item_prefix,
                        &mut item_index,
                        TimelineItemKind::ToolCall,
                        tool_rest,
                        if running { "streaming" } else { "complete" },
                    );
                    break;
                }
            }
        }
    }

    if items.is_empty() && running {
        items.push(TimelineItem {
            id: format!("{item_prefix}-assistant-waiting"),
            kind: TimelineItemKind::AssistantMessage,
            text: String::new(),
            status: "streaming".to_string(),
        });
    }

    items
}

enum CanonicalInvocationSegment<'a> {
    Parsed { raw: &'a str, consumed: usize },
    Incomplete,
    Invalid,
}

fn extract_canonical_invocation_segment(text: &str) -> CanonicalInvocationSegment<'_> {
    let prefix = if text.starts_with("TOOL:") {
        "TOOL:"
    } else if text.starts_with("ACTION:") {
        "ACTION:"
    } else {
        return CanonicalInvocationSegment::Invalid;
    };

    let Some(rest_with_ws) = text.strip_prefix(prefix) else {
        return CanonicalInvocationSegment::Invalid;
    };
    let rest = rest_with_ws.trim_start();
    let Some(separator_idx) = rest.find(|c: char| c.is_whitespace() || c == '{') else {
        return CanonicalInvocationSegment::Incomplete;
    };

    let name = &rest[..separator_idx];
    if name.is_empty()
        || !name.chars().all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-' | '.')
        })
    {
        return CanonicalInvocationSegment::Invalid;
    }

    let payload_with_ws = &rest[separator_idx..];
    let payload = payload_with_ws.trim_start();
    if payload.is_empty() {
        return CanonicalInvocationSegment::Incomplete;
    }
    if !payload.starts_with('{') {
        return CanonicalInvocationSegment::Invalid;
    }

    let Some(json_end_rel) = first_balanced_json_object_end(payload) else {
        return CanonicalInvocationSegment::Incomplete;
    };

    if serde_json::from_str::<serde_json::Value>(&payload[..json_end_rel]).is_err() {
        return CanonicalInvocationSegment::Invalid;
    }

    let leading_ws_after_prefix = rest_with_ws.len() - rest.len();
    let ws_before_payload = payload_with_ws.len() - payload.len();
    let consumed =
        prefix.len() + leading_ws_after_prefix + separator_idx + ws_before_payload + json_end_rel;

    CanonicalInvocationSegment::Parsed {
        raw: &text[..consumed],
        consumed,
    }
}

fn first_balanced_json_object_end(payload: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in payload.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(idx + ch.len_utf8());
                }
            }
            _ => {}
        }
    }

    None
}

pub(super) fn looks_like_syscall_invocation(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }

    for prefix in [
        "TOOL:",
        "ACTION:",
        "SEND:",
        "SPAWN:",
        "PYTHON:",
        "WRITE_FILE:",
        "READ_FILE:",
        "CALC:",
    ] {
        if trimmed.starts_with(prefix) {
            return true;
        }
    }

    trimmed == "LS" || trimmed.starts_with("LS ")
}

fn timeline_item_kind_for_invocation(content: &str) -> TimelineItemKind {
    let trimmed = content.trim();
    if trimmed.starts_with("ACTION:")
        || trimmed.starts_with("SPAWN:")
        || trimmed.starts_with("SEND:")
    {
        TimelineItemKind::ActionCall
    } else {
        TimelineItemKind::ToolCall
    }
}

fn push_timeline_text_item(
    items: &mut Vec<TimelineItem>,
    item_prefix: &str,
    item_index: &mut usize,
    kind: TimelineItemKind,
    text: &str,
    status: &str,
) {
    let normalized = text.trim();
    if normalized.is_empty() {
        return;
    }

    items.push(TimelineItem {
        id: format!("{item_prefix}-segment-{}", *item_index),
        kind,
        text: normalized.to_string(),
        status: status.to_string(),
    });
    *item_index += 1;
}
