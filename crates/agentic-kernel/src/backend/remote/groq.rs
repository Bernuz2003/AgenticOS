use super::openai_compatible::{RemoteOpenAIProviderProfile, RemoteOpenAITransport};

pub(super) const GROQ_RESPONSES_PROFILE: RemoteOpenAIProviderProfile = RemoteOpenAIProviderProfile {
    backend_id: "groq-responses",
    display_name: "Groq Responses",
    config_section: "groq_responses",
    endpoint_env: "AGENTIC_GROQ_ENDPOINT",
    api_key_env: "AGENTIC_GROQ_API_KEY",
    fallback_api_key_env: Some("GROQ_API_KEY"),
    transport: RemoteOpenAITransport::ResponsesApi,
};
