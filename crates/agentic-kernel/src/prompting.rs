use minijinja::{context, Environment, UndefinedBehavior};
use serde::Serialize;

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
        let generation = &crate::config::kernel_config().generation;
        let profile = match family {
            PromptFamily::Llama => &generation.llama,
            PromptFamily::Qwen => &generation.qwen,
            PromptFamily::Mistral => &generation.mistral,
            PromptFamily::Unknown => &generation.unknown,
        };

        Self {
            temperature: profile.temperature,
            top_p: profile.top_p,
            seed: profile.seed,
            max_tokens: profile.max_tokens,
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
        return render_single_chat_turn(family, metadata, template, "system", content)
            .unwrap_or_else(|| fallback_system_turn(content, family));
    }

    fallback_system_turn(content, family)
}

pub fn format_user_message_with_metadata(
    content: &str,
    family: PromptFamily,
    metadata: Option<&ModelMetadata>,
) -> String {
    if let Some(template) = metadata.and_then(|meta| meta.chat_template.as_deref()) {
        return render_single_chat_turn(family, metadata, template, "user", content)
            .unwrap_or_else(|| fallback_user_turn(content, family));
    }

    fallback_user_turn(content, family)
}

pub fn format_interprocess_user_message_with_metadata(
    from_pid: u64,
    message: &str,
    family: PromptFamily,
    metadata: Option<&ModelMetadata>,
) -> String {
    let content = format!("[Message from PID {}]: {}", from_pid, message);
    if let Some(template) = metadata.and_then(|meta| meta.chat_template.as_deref()) {
        return render_single_chat_turn(family, metadata, template, "user", &content)
            .unwrap_or_else(|| fallback_user_turn(&content, family));
    }

    fallback_user_turn(&content, family)
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

fn render_single_chat_turn(
    family: PromptFamily,
    metadata: Option<&ModelMetadata>,
    template: &str,
    role: &str,
    content: &str,
) -> Option<String> {
    if looks_like_jinja(template) {
        return render_jinja_chat_template(template, &[ChatMessage { role, content }], metadata);
    }

    Some(render_placeholder_chat_turn(
        role, content, family, metadata, template,
    ))
}

fn render_placeholder_chat_turn(
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

fn render_jinja_chat_template(
    template: &str,
    messages: &[ChatMessage<'_>],
    metadata: Option<&ModelMetadata>,
) -> Option<String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Lenient);
    env.add_template("chat", template).ok()?;

    let template = env.get_template("chat").ok()?;
    let special_tokens = metadata.and_then(|meta| meta.special_tokens.as_ref());

    template
        .render(context! {
            messages => messages,
            add_generation_prompt => true,
            bos_token => special_tokens.and_then(|tokens| tokens.get("bos")).cloned().unwrap_or_default(),
            eos_token => special_tokens.and_then(|tokens| tokens.get("eos")).cloned().unwrap_or_default(),
            eot_token => special_tokens.and_then(|tokens| tokens.get("eot")).cloned().unwrap_or_default(),
            pad_token => special_tokens.and_then(|tokens| tokens.get("pad")).cloned().unwrap_or_default(),
            assistant_preamble => metadata.and_then(|meta| meta.assistant_preamble.clone()).unwrap_or_default(),
            enable_thinking => true,
            tools => Vec::<String>::new(),
            documents => Vec::<String>::new(),
        })
        .ok()
}

fn looks_like_jinja(template: &str) -> bool {
    template.contains("{{") || template.contains("{%")
}

fn fallback_system_turn(content: &str, family: PromptFamily) -> String {
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

fn fallback_user_turn(content: &str, family: PromptFamily) -> String {
    match family {
        PromptFamily::Llama => format!(
            "<|begin_of_text|><|start_header_id|>user<|end_header_id|>\n\n{}\n<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n",
            content
        ),
        PromptFamily::Qwen => format!(
            "<|im_start|>user\n{}\n<|im_end|>\n<|im_start|>assistant\n",
            content
        ),
        PromptFamily::Mistral => format!("[INST] {} [/INST]", content),
        PromptFamily::Unknown => format!("\n[user]\n{}\n[/user]\n", content),
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
        format_interprocess_user_message_with_metadata, format_system_injection_with_metadata,
        format_user_message_with_metadata, should_stop_on_text_with_metadata, PromptFamily,
    };

    #[test]
    fn qwen_stop_markers_are_detected() {
        assert!(should_stop_on_text_with_metadata(
            PromptFamily::Qwen,
            "...<|im_end|>...",
            None
        ));
        assert!(should_stop_on_text_with_metadata(
            PromptFamily::Qwen,
            "...<|endoftext|>...",
            None
        ));
        assert!(!should_stop_on_text_with_metadata(
            PromptFamily::Qwen,
            "plain text without stop marker",
            None
        ));
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

        let rendered =
            format_system_injection_with_metadata("hello", PromptFamily::Llama, Some(&metadata));
        assert_eq!(rendered, "<system>hello</system><assistant>");

        let user_rendered =
            format_user_message_with_metadata("ciao", PromptFamily::Llama, Some(&metadata));
        assert_eq!(user_rendered, "<user>ciao</user><assistant>");
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
        assert!(should_stop_on_text_with_metadata(
            PromptFamily::Qwen,
            "...<|im_end|>...",
            None
        ));
    }

    #[test]
    fn initial_user_message_uses_family_chat_format() {
        let qwen = format_user_message_with_metadata("ciao", PromptFamily::Qwen, None);
        assert!(qwen.contains("<|im_start|>user"));
        assert!(qwen.contains("<|im_start|>assistant"));

        let inter =
            format_interprocess_user_message_with_metadata(7, "pong", PromptFamily::Qwen, None);
        assert!(inter.contains("[Message from PID 7]: pong"));
        assert!(inter.contains("<|im_start|>assistant"));
    }

    #[test]
    fn jinja_chat_template_renders_messages_and_generation_prompt() {
        let metadata = ModelMetadata {
            chat_template: Some(
                "{% for message in messages %}<{{ message.role }}>{{ message.content }}</{{ message.role }}>{% endfor %}{% if add_generation_prompt %}<assistant>{% endif %}".to_string(),
            ),
            ..Default::default()
        };

        let rendered =
            format_user_message_with_metadata("dimmi ciao", PromptFamily::Qwen, Some(&metadata));

        assert_eq!(rendered, "<user>dimmi ciao</user><assistant>");
    }
}
