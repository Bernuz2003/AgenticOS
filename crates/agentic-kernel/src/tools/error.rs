use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ToolError {
    #[error("Malformed tool invocation: {0}")]
    MalformedInvocation(String),

    #[error("Tool '{0}' not found")]
    NotFound(String),

    #[error("Tool '{0}' is disabled")]
    Disabled(String),

    #[error("Invalid input for tool '{0}': {1}")]
    InvalidInput(String, String),

    #[error("Schema violation for tool '{0}': {1}")]
    SchemaViolation(String, String),

    #[error("Output schema violation for tool '{0}': {1}")]
    OutputSchemaViolation(String, String),

    #[error("Execution policy denied for '{0}': {1}")]
    PolicyDenied(String, String),

    #[error("Rate limited: {0}")]
    RateLimited(String),

    #[error("Execution timeout for '{0}': {1}ms")]
    Timeout(String, u64),

    #[error("Backend unavailable for '{0}': {1}")]
    BackendUnavailable(String, String),

    #[error("Tool execution failed for '{0}': {1}")]
    ExecutionFailed(String, String),

    #[error("Internal tool registry error: {0}")]
    Internal(String),
}
