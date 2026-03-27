//! Agent orchestration — DAG-based multi-task workflow engine.
//!
//! Provides kernel-side logic for structured multi-agent workflows.
//! An orchestration is a directed acyclic graph (DAG) of tasks where each
//! task is an LLM prompt execution. Dependencies define data-flow: task
//! artifacts from completed upstream nodes are injected as context into
//! successor tasks.

mod artifacts;
mod failure_policy;
mod graph;
mod output;
#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
mod transitions;
mod types;
mod validation;

use std::collections::HashMap;

use crate::errors::OrchestratorError;
use crate::policy::workload_from_label_or_default;

use artifacts::refresh_output_metrics;
use graph::build_spawn_request;
use output::{append_with_cap, build_task_prompt};
pub use types::{
    FailurePolicy, Orchestration, Orchestrator, RetryPlan, RunningTaskOutput, SpawnRequest,
    TaskArtifact, TaskAttemptFinalization, TaskGraphDef, TaskInputArtifact, TaskNodeDef,
    TaskPidBinding, TaskStatus,
};
use validation::validate_and_sort;
