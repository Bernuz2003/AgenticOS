/// Centralized, typed error hierarchy for the AgenticOS Kernel.
///
/// Replaces ad-hoc `String` errors throughout the codebase with matchable,
/// structured variants. Migration is incremental — modules can adopt these
/// types at their own pace while the `String`-based APIs remain available
/// through `Display` / `.to_string()`.
use thiserror::Error;

// ── Top-level kernel error ──────────────────────────────────────────────

// TODO: adopt KernelError as the return type in Kernel::run() and command handlers.
#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum KernelError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Memory(#[from] MemoryError),

    #[error("{0}")]
    Engine(#[from] EngineError),

    #[error("{0}")]
    Protocol(#[from] ProtocolError),

    #[error("{0}")]
    Catalog(#[from] CatalogError),

    #[error("{0}")]
    Orchestrator(#[from] OrchestratorError),

    #[error("{0}")]
    Config(String),
}

// ── Memory subsystem errors ─────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("NeuralMemory: PID {pid} requested {requested} token slots > quota {quota}")]
    QuotaExceeded {
        pid: u64,
        requested: usize,
        quota: usize,
    },

    #[error("NeuralMemory: token_slots must be > 0")]
    ZeroTokenSlots,

    #[error("NeuralMemory: PID {0} is not registered")]
    PidNotRegistered(u64),

    #[error("Swap error: {0}")]
    Swap(String),
}

// ── Engine / process errors ─────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("No model loaded")]
    NoModelLoaded,

    #[error("PID {0} not found")]
    PidNotFound(u64),

    #[error("Master model not loaded")]
    MasterModelMissing,

    #[error("Spawn failed: {0}")]
    SpawnFailed(String),

    #[error("Inference error PID {pid}: {detail}")]
    Inference { pid: u64, detail: String },

    #[error("Backend error: {0}")]
    Backend(String),
}

// ── Protocol errors ─────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Unsupported protocol version(s): {0}")]
    UnsupportedProtocolVersion(String),

    #[error("Missing required capability: {0}")]
    MissingCapability(String),

    #[error("Invalid protocol JSON: {0}")]
    InvalidProtocolJson(String),
}

// ── Orchestrator errors ────────────────────────────────────────────────

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum OrchestratorError {
    #[error("task graph is empty")]
    EmptyGraph,

    #[error("duplicate task id '{0}'")]
    DuplicateTaskId(String),

    #[error("task '{0}' depends on itself")]
    SelfDependency(String),

    #[error("task '{task}' depends on unknown task '{dependency}'")]
    UnknownDependency { task: String, dependency: String },

    #[error("task graph contains a cycle")]
    CycleDetected,
}

// ── Model catalog errors ────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("Model '{0}' not found in catalog")]
    ModelNotFound(String),

    #[error("Model directory read failed '{path}': {detail}")]
    DirectoryReadFailed { path: String, detail: String },

    #[error("No model selected. Use SELECT_MODEL first or pass a model path/id to LOAD.")]
    NoModelSelected,

    #[error("Invalid model selector '{0}'. Use model id from LIST_MODELS or provide .gguf path.")]
    InvalidSelector(String),

    #[error("Model path not found: {0}")]
    PathNotFound(String),

    #[error("Model driver resolution failed: {0}")]
    DriverResolutionFailed(String),
}
