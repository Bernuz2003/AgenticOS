use super::openai_compatible::{RemoteOpenAIProviderProfile, RemoteOpenAITransport};

pub(super) const OPENROUTER_PROFILE: RemoteOpenAIProviderProfile = RemoteOpenAIProviderProfile {
    backend_id: "openrouter",
    display_name: "OpenRouter",
    config_section: "openrouter",
    endpoint_env: "AGENTIC_OPENROUTER_ENDPOINT",
    api_key_env: "AGENTIC_OPENROUTER_API_KEY",
    fallback_api_key_env: Some("OPENROUTER_API_KEY"),
    transport: RemoteOpenAITransport::ChatCompletions,
};
