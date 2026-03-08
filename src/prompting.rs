use crate::model_catalog::ModelMetadata;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptFamily {
    Llama,
    Qwen,
    Mistral,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct GenerationConfig {
    pub temperature: f64,
    pub top_p: f64,
    pub seed: u64,
    pub max_tokens: usize,
}

impl GenerationConfig {
    pub fn defaults_for(family: PromptFamily) -> Self {
        match family {
            PromptFamily::Llama => Self {
                temperature: 0.7,
                top_p: 0.9,
                seed: 299_792_458,
                max_tokens: 500,
            },
            PromptFamily::Qwen => Self {
                temperature: 0.7,
                top_p: 0.9,
                seed: 299_792_458,
                max_tokens: 500,
            },
            PromptFamily::Mistral => Self {
                temperature: 0.7,
                top_p: 0.92,
                seed: 299_792_458,
                max_tokens: 500,
            },
            PromptFamily::Unknown => Self {
                temperature: 0.7,
                top_p: 0.9,
                seed: 299_792_458,
                max_tokens: 500,
            },
        }
    }
}

pub fn should_stop_on_text_with_metadata(
    family: PromptFamily,
    text: &str,
    metadata: Option<&ModelMetadata>,
) -> bool {
    if text.contains("]]") {
        return true;
    }

    if let Some(markers) = metadata.and_then(|meta| meta.stop_markers.as_ref()) {
        if markers.iter().any(|marker| text.contains(marker)) {
            return true;
        }
    }

    if let Some(tokens) = metadata.and_then(|meta| meta.special_tokens.as_ref()) {
        let dynamic_markers: Vec<&str> = tokens
            .iter()
            .filter_map(|(key, value)| {
                let lowered = key.to_ascii_lowercase();
                if lowered.contains("eos")
                    || lowered.contains("eot")
                    || lowered.contains("stop")
                    || lowered.contains("end")
                {
                    Some(value.as_str())
                } else {
                    None
                }
            })
            .collect();
        if dynamic_markers.iter().any(|marker| text.contains(marker)) {
            return true;
        }
    }

    let markers: &[&str] = match family {
        PromptFamily::Llama => &["<|eot_id|>", "<|end_of_text|>"],
        PromptFamily::Qwen => &["<|im_end|>", "<|endoftext|>"],
        PromptFamily::Mistral => &["</s>"],
        PromptFamily::Unknown => &[],
    };

    markers.iter().any(|marker| text.contains(marker))
}

pub fn format_system_injection_with_metadata(
    content: &str,
    family: PromptFamily,
    metadata: Option<&ModelMetadata>,
) -> String {
    if let Some(template) = metadata.and_then(|meta| meta.chat_template.as_deref()) {
        return render_chat_turn("system", content, family, metadata, template);
    }

    match family {
        PromptFamily::Llama => format!(
            "<|eot_id|><|start_header_id|>system<|end_header_id|>\n\n{}\n<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n",
            content
        ),
        PromptFamily::Qwen => format!(
            "<|im_start|>system\n{}\n<|im_end|>\n<|im_start|>assistant\n",
            content
        ),
        PromptFamily::Mistral => format!("[INST] [SYSTEM] {} [/SYSTEM] [/INST]", content),
        PromptFamily::Unknown => format!("\n[system]\n{}\n[/system]\n", content),
    }
}

pub fn format_interprocess_user_message_with_metadata(
    from_pid: u64,
    message: &str,
    family: PromptFamily,
    metadata: Option<&ModelMetadata>,
) -> String {
    if let Some(template) = metadata.and_then(|meta| meta.chat_template.as_deref()) {
        let content = format!("[Message from PID {}]: {}", from_pid, message);
        return render_chat_turn("user", &content, family, metadata, template);
    }

    match family {
        PromptFamily::Llama => format!(
            "<|eot_id|><|start_header_id|>user<|end_header_id|>\n\n[Message from PID {}]: {}\n<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n",
            from_pid, message
        ),
        PromptFamily::Qwen => format!(
            "<|im_start|>user\n[Message from PID {}]: {}\n<|im_end|>\n<|im_start|>assistant\n",
            from_pid, message
        ),
        PromptFamily::Mistral => {
            format!("[INST] [Message from PID {}]: {} [/INST]", from_pid, message)
        }
        PromptFamily::Unknown => format!("\n[user]\n[Message from PID {}]: {}\n[/user]\n", from_pid, message),
    }
}

fn render_chat_turn(
    role: &str,
    content: &str,
    family: PromptFamily,
    metadata: Option<&ModelMetadata>,
    template: &str,
) -> String {
    let assistant_preamble = metadata
        .and_then(|meta| meta.assistant_preamble.clone())
        .unwrap_or_else(|| default_assistant_preamble(family));

    let rendered = template
        .replace("{role}", role)
        .replace("{content}", content)
        .replace("{assistant_preamble}", &assistant_preamble);

    if template.contains("{assistant_preamble}") {
        rendered
    } else {
        format!("{}{}", rendered, assistant_preamble)
    }
}

fn default_assistant_preamble(family: PromptFamily) -> String {
    match family {
        PromptFamily::Llama => {
            "<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n".to_string()
        }
        PromptFamily::Qwen => "<|im_start|>assistant\n".to_string(),
        PromptFamily::Mistral | PromptFamily::Unknown => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::model_catalog::ModelMetadata;

    use super::{
        format_system_injection_with_metadata,
        should_stop_on_text_with_metadata,
        PromptFamily,
    };

    #[test]
    fn qwen_stop_markers_are_detected() {
        assert!(should_stop_on_text_with_metadata(PromptFamily::Qwen, "...<|im_end|>...", None));
        assert!(should_stop_on_text_with_metadata(PromptFamily::Qwen, "...<|endoftext|>...", None));
        assert!(!should_stop_on_text_with_metadata(PromptFamily::Qwen, "plain text without stop marker", None));
    }

    #[test]
    fn llama_and_qwen_system_templates_include_expected_tokens() {
        let llama = format_system_injection_with_metadata("hello", PromptFamily::Llama, None);
        assert!(llama.contains("<|start_header_id|>system<|end_header_id|>"));
        assert!(llama.contains("<|eot_id|>"));

        let qwen = format_system_injection_with_metadata("hello", PromptFamily::Qwen, None);
        assert!(qwen.contains("<|im_start|>system"));
        assert!(qwen.contains("<|im_end|>"));
    }

    #[test]
    fn metadata_template_overrides_family_template() {
        let metadata = ModelMetadata {
            chat_template: Some("<{role}>{content}</{role}>{assistant_preamble}".to_string()),
            assistant_preamble: Some("<assistant>".to_string()),
            ..Default::default()
        };

        let rendered = format_system_injection_with_metadata(
            "hello",
            PromptFamily::Llama,
            Some(&metadata),
        );
        assert_eq!(rendered, "<system>hello</system><assistant>");
    }

    #[test]
    fn metadata_stop_markers_override_family_defaults() {
        let mut special_tokens = BTreeMap::new();
        special_tokens.insert("eot".to_string(), "<stop_here>".to_string());
        let metadata = ModelMetadata {
            stop_markers: Some(vec!["<stop_here>".to_string()]),
            special_tokens: Some(special_tokens),
            ..Default::default()
        };

        assert!(should_stop_on_text_with_metadata(
            PromptFamily::Unknown,
            "abc<stop_here>def",
            Some(&metadata),
        ));
        assert!(should_stop_on_text_with_metadata(PromptFamily::Qwen, "...<|im_end|>...", None));
    }
}
