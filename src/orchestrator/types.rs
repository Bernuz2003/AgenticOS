use serde::Deserialize;
use std::collections::HashMap;
use std::time::Instant;

use crate::model_catalog::WorkloadClass;

/// Failure policy for an orchestration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    FailFast,
    BestEffort,
}

impl Default for FailurePolicy {
    fn default() -> Self {
        Self::FailFast
    }
}

/// A single task node definition (JSON-deserializable).
#[derive(Debug, Clone, Deserialize)]
pub struct TaskNodeDef {
    pub id: String,
    pub prompt: String,
    #[serde(default)]
    pub workload: Option<String>,
    #[serde(default)]
    pub deps: Vec<String>,
}

/// Full task-graph payload (JSON-deserializable).
#[derive(Debug, Clone, Deserialize)]
pub struct TaskGraphDef {
    pub tasks: Vec<TaskNodeDef>,
    #[serde(default)]
    pub failure_policy: FailurePolicy,
}

/// Runtime status of a single task within an orchestration.
#[derive(Debug, Clone)]
pub enum TaskStatus {
    Pending,
    Running { pid: u64 },
    Completed,
    Failed { error: String },
    Skipped,
}

impl TaskStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed { .. } | Self::Skipped)
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Running { .. } => "running",
            Self::Completed => "completed",
            Self::Failed { .. } => "failed",
            Self::Skipped => "skipped",
        }
    }
}

/// A live orchestration instance.
pub struct Orchestration {
    pub owner_id: usize,
    pub failure_policy: FailurePolicy,
    pub tasks: HashMap<String, TaskNodeDef>,
    pub topo_order: Vec<String>,
    pub status: HashMap<String, TaskStatus>,
    pub output: HashMap<String, String>,
    pub truncated_outputs: usize,
    pub output_chars_stored: usize,
    pub created_at: Instant,
}

impl Orchestration {
    pub fn new(
        owner_id: usize,
        failure_policy: FailurePolicy,
        tasks: HashMap<String, TaskNodeDef>,
        topo_order: Vec<String>,
        status: HashMap<String, TaskStatus>,
    ) -> Self {
        Self {
            owner_id,
            failure_policy,
            tasks,
            topo_order,
            status,
            output: HashMap::new(),
            truncated_outputs: 0,
            output_chars_stored: 0,
            created_at: Instant::now(),
        }
    }

    pub fn is_finished(&self) -> bool {
        self.status.values().all(|status| status.is_terminal())
    }

    /// `(pending, running, completed, failed, skipped)`
    pub fn counts(&self) -> (usize, usize, usize, usize, usize) {
        let (mut pending, mut running, mut completed, mut failed, mut skipped) = (0, 0, 0, 0, 0);
        for status in self.status.values() {
            match status {
                TaskStatus::Pending => pending += 1,
                TaskStatus::Running { .. } => running += 1,
                TaskStatus::Completed => completed += 1,
                TaskStatus::Failed { .. } => failed += 1,
                TaskStatus::Skipped => skipped += 1,
            }
        }
        (pending, running, completed, failed, skipped)
    }

    pub(crate) fn running_pids(&self) -> Vec<u64> {
        self.status
            .values()
            .filter_map(|status| match status {
                TaskStatus::Running { pid } => Some(*pid),
                _ => None,
            })
            .collect()
    }
}

/// Request from the orchestrator for the runtime to spawn a new task.
#[derive(Debug)]
pub struct SpawnRequest {
    pub orch_id: u64,
    pub task_id: String,
    pub prompt: String,
    pub workload: WorkloadClass,
    pub owner_id: usize,
}

/// Manages all active orchestrations.
pub struct Orchestrator {
    pub(crate) orchestrations: HashMap<u64, Orchestration>,
    pub(crate) next_id: u64,
    pub(crate) pid_to_task: HashMap<u64, (u64, String)>,
    pub(crate) max_output_chars: usize,
}