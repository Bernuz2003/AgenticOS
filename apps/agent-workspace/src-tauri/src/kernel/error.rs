use thiserror::Error;

#[derive(Debug, Error)]
pub enum KernelBridgeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol framing error: {0}")]
    ProtocolParse(#[from] agentic_protocol::ProtocolParseError),

    #[error("JSON decode error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("UTF-8 decode error: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("Timed out waiting for {0}")]
    TimedOut(&'static str),

    #[error("Kernel connection closed")]
    ConnectionClosed,

    #[error("Malformed kernel response header")]
    MalformedResponseHeader,

    #[error("Invalid payload length in kernel response")]
    InvalidPayloadLength,

    #[error("Kernel payload missing protocol envelope for schema(s): {expected}")]
    MissingProtocolEnvelope { expected: String },

    #[error("Protocol envelope did not contain data")]
    MissingEnvelopeData,

    #[error("Unexpected kernel schema '{received}', expected one of: {expected}")]
    UnexpectedSchema { received: String, expected: String },

    #[error("Kernel returned error {code}: {message}")]
    KernelRejected { code: String, message: String },

    #[error("Kernel connection is not available")]
    ConnectionUnavailable,
}

pub type KernelBridgeResult<T> = Result<T, KernelBridgeError>;
