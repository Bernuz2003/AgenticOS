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
mod kernel_repo;
mod migrations;
mod recovery;
mod runtime;
mod runtime_queue;
mod scheduled_jobs;
mod service;
mod sessions_repo;
mod timeline;
mod workflow_artifacts;

pub(crate) use accounting::StoredAccountingEvent;
pub(crate) use audit::NewAuditEvent;
pub(crate) use recovery::BootRecoveryReport;
pub(crate) use runtime::StoredRuntimeRecord;
pub(crate) use runtime_queue::StoredRuntimeLoadQueueEntry;
pub(crate) use scheduled_jobs::{NewScheduledJobRecord, StoredScheduledJob, StoredScheduledJobRun};
pub(crate) use service::{current_timestamp_ms, StorageError, StorageService, StoredSessionRecord};
pub(crate) use timeline::StoredReplayMessage;
pub(crate) use workflow_artifacts::{
    StoredWorkflowArtifact, StoredWorkflowArtifactInput, StoredWorkflowTaskAttempt,
    WorkflowArtifactInputRef,
};
