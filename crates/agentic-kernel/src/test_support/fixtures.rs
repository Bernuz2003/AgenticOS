use crate::prompting::{GenerationConfig, PromptFamily};

pub(crate) fn default_generation_config() -> GenerationConfig {
    GenerationConfig::defaults_for(PromptFamily::Unknown)
}

pub(crate) fn sample_allowed_tools() -> Vec<String> {
    vec![
        "python".to_string(),
        "read_file".to_string(),
        "list_files".to_string(),
        "calc".to_string(),
    ]
}
