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

pub fn should_stop_on_text(family: PromptFamily, text: &str) -> bool {
    if text.contains("]]") {
        return true;
    }

    let markers: &[&str] = match family {
        PromptFamily::Llama => &["<|eot_id|>", "<|end_of_text|>"],
        PromptFamily::Qwen => &["<|im_end|>", "<|endoftext|>"],
        PromptFamily::Mistral => &["</s>"],
        PromptFamily::Unknown => &[],
    };

    markers.iter().any(|marker| text.contains(marker))
}

pub fn format_system_injection(content: &str, family: PromptFamily) -> String {
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

pub fn format_interprocess_user_message(from_pid: u64, message: &str, family: PromptFamily) -> String {
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

#[cfg(test)]
mod tests {
    use super::{format_system_injection, should_stop_on_text, PromptFamily};

    #[test]
    fn qwen_stop_markers_are_detected() {
        assert!(should_stop_on_text(PromptFamily::Qwen, "...<|im_end|>..."));
        assert!(should_stop_on_text(PromptFamily::Qwen, "...<|endoftext|>..."));
        assert!(!should_stop_on_text(PromptFamily::Qwen, "plain text without stop marker"));
    }

    #[test]
    fn llama_and_qwen_system_templates_include_expected_tokens() {
        let llama = format_system_injection("hello", PromptFamily::Llama);
        assert!(llama.contains("<|start_header_id|>system<|end_header_id|>"));
        assert!(llama.contains("<|eot_id|>"));

        let qwen = format_system_injection("hello", PromptFamily::Qwen);
        assert!(qwen.contains("<|im_start|>system"));
        assert!(qwen.contains("<|im_end|>"));
    }
}
