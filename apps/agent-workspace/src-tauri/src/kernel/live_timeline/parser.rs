use crate::models::kernel::{TimelineItem, TimelineItemKind};

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
        let next_marker = find_next_think_marker(remaining);

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
            Some(offset) => {
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

fn find_next_think_marker(stream: &str) -> Option<usize> {
    let mut in_fenced_block = false;
    let mut absolute_offset = 0usize;

    for line in stream.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let leading_ws = line.len() - trimmed.len();
        if !in_fenced_block && trimmed.starts_with("<think>") {
            return Some(absolute_offset + leading_ws);
        }

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fenced_block = !in_fenced_block;
        }

        absolute_offset += line.len();
    }

    None
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
