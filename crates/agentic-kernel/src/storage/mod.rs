//! Storage Layer and Control Plane
//!
//! AgenticOS uses SQLite (`agenticos.db`) as the authoritative Single Source of Truth
//! for the system's Control Plane. All critical state—including persistent Sessions,
//! historical Execution runs (boot ID, PID mappings), Audit logs, and Accounting—lives
//! here.
//!
//! Architectural Invariant: We do not maintain complex distributed or duplicated state in
//! memory that could desynchronize on crash. Operations are transactional, and memory
//! structures (like `SessionRegistry` or `ProcessRegistry`) are essentially caches or
//! fast paths that can always be deterministically reconstructed from SQLite.

mod accounting;
mod audit;
mod conversation;
mod ipc;
mod schema;
mod workflows;

pub(crate) use accounting::StoredAccountingEvent;
pub(crate) use audit::NewAuditEvent;
pub(crate) use conversation::StoredReplayMessage;
pub(crate) use conversation::StoredSessionRecord;
pub(crate) use ipc::{IpcMailboxSelector, NewIpcMessage, StoredIpcMessage};
#[allow(unused_imports)]
pub(crate) use schema::{
    current_timestamp_ms, BootRecoveryReport, KernelBootRecord, StorageError, StorageService,
};
pub(crate) use workflows::{NewScheduledJobRecord, StoredScheduledJob, StoredScheduledJobRun};
pub(crate) use workflows::{
    StoredWorkflowArtifact, StoredWorkflowArtifactInput, StoredWorkflowTaskAttempt,
    WorkflowArtifactInputRef,
};
