use std::collections::BTreeMap;

use crate::model_catalog::ModelMetadata;

use super::{
    format_initial_prompt_with_metadata, format_interprocess_user_message_with_metadata,
    format_system_injection_with_metadata, format_user_message_with_metadata,
    should_stop_on_text_with_metadata, PromptFamily,
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

    let inter = format_interprocess_user_message_with_metadata(7, "pong", PromptFamily::Qwen, None);
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

#[test]
fn initial_prompt_combines_system_and_user_without_duplicate_assistant_turns() {
    let rendered = format_initial_prompt_with_metadata(
        Some("system policy"),
        "do the task",
        PromptFamily::Qwen,
        None,
    );

    assert!(rendered.contains("<|im_start|>system"));
    assert!(rendered.contains("<|im_start|>user"));
    assert_eq!(rendered.matches("<|im_start|>assistant").count(), 1);
}

#[test]
fn initial_prompt_uses_metadata_template_for_multiple_messages() {
    let metadata = ModelMetadata {
        chat_template: Some(
            "{% for message in messages %}<{{ message.role }}>{{ message.content }}</{{ message.role }}>{% endfor %}{% if add_generation_prompt %}<assistant>{% endif %}".to_string(),
        ),
        ..Default::default()
    };

    let rendered = format_initial_prompt_with_metadata(
        Some("policy"),
        "question",
        PromptFamily::Qwen,
        Some(&metadata),
    );

    assert_eq!(
        rendered,
        "<system>policy</system><user>question</user><assistant>"
    );
}
