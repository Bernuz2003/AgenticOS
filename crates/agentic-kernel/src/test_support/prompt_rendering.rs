use crate::model_catalog::ModelMetadata;
use crate::prompting::{format_initial_prompt_with_metadata, PromptFamily};

pub fn render_qwen_initial_prompt_with_template(
    chat_template: &str,
    system_content: &str,
    user_content: &str,
) -> String {
    let metadata = ModelMetadata {
        chat_template: Some(chat_template.to_string()),
        ..Default::default()
    };

    format_initial_prompt_with_metadata(
        Some(system_content),
        user_content,
        PromptFamily::Qwen,
        Some(&metadata),
    )
}
