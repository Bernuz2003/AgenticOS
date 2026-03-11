//! Agent orchestration — DAG-based multi-task workflow engine.
//!
//! Provides kernel-side logic for structured multi-agent workflows.
//! An orchestration is a directed acyclic graph (DAG) of tasks where each
//! task is an LLM prompt execution. Dependencies define data-flow: the
//! output of completed tasks is injected as context into successor tasks.
//!
//! This module does **not** spawn LLM processes — it produces
//! [`SpawnRequest`] values consumed by the runtime loop to call
//! `LLMEngine::spawn_process`.

mod output;
#[cfg(test)]
mod tests;
mod types;
mod validation;

use std::collections::HashMap;

use crate::errors::OrchestratorError;
use crate::policy::workload_from_label_or_default;

use output::{append_with_cap, build_task_prompt};
pub use types::{
    FailurePolicy, Orchestration, Orchestrator, SpawnRequest, TaskGraphDef, TaskNodeDef, TaskStatus,
};
use validation::validate_and_sort;

impl Orchestrator {
    pub fn new() -> Self {
        Self {
            orchestrations: HashMap::new(),
            next_id: 1,
            pid_to_task: HashMap::new(),
            max_output_chars: crate::config::kernel_config().orchestrator.max_output_chars,
        }
    }

    /// Validate and register a new task graph.
    /// Returns `(orchestration_id, initial_spawn_requests)` for root tasks.
    pub fn register(
        &mut self,
        graph: TaskGraphDef,
        owner_id: usize,
    ) -> Result<(u64, Vec<SpawnRequest>), OrchestratorError> {
        let topo_order = validate_and_sort(&graph.tasks)?;

        let orch_id = self.next_id;
        self.next_id += 1;

        let tasks: HashMap<String, TaskNodeDef> = graph
            .tasks
            .iter()
            .map(|task| (task.id.clone(), task.clone()))
            .collect();
        let status: HashMap<String, TaskStatus> = graph
            .tasks
            .iter()
            .map(|task| (task.id.clone(), TaskStatus::Pending))
            .collect();

        let mut spawn_requests = Vec::new();
        for task_id in &topo_order {
            let Some(task) = tasks.get(task_id) else {
                continue;
            };
            if task.deps.is_empty() {
                spawn_requests.push(SpawnRequest {
                    orch_id,
                    task_id: task_id.clone(),
                    prompt: task.prompt.clone(),
                    workload: workload_from_label_or_default(task.workload.as_deref()),
                    owner_id,
                    context_policy: task.resolved_context_policy(),
                });
            }
        }

        self.orchestrations.insert(
            orch_id,
            Orchestration::new(owner_id, graph.failure_policy, tasks, topo_order, status),
        );

        Ok((orch_id, spawn_requests))
    }

    /// Register a spawned PID for a task.
    pub fn register_pid(&mut self, pid: u64, orch_id: u64, task_id: &str) {
        self.pid_to_task.insert(pid, (orch_id, task_id.to_string()));
        if let Some(orch) = self.orchestrations.get_mut(&orch_id) {
            orch.status
                .insert(task_id.to_string(), TaskStatus::Running { pid });
        }
    }

    /// Mark a task whose spawn failed (memory admission, etc.).
    pub fn mark_spawn_failed(&mut self, orch_id: u64, task_id: &str, error: &str) {
        if let Some(orch) = self.orchestrations.get_mut(&orch_id) {
            orch.status.insert(
                task_id.to_string(),
                TaskStatus::Failed {
                    error: error.to_string(),
                },
            );
        }
    }

    /// Check if a PID belongs to an orchestration.
    pub fn is_orchestrated(&self, pid: u64) -> bool {
        self.pid_to_task.contains_key(&pid)
    }

    /// Append generated text to a task's output buffer.
    pub fn append_output(&mut self, pid: u64, text: &str) {
        if let Some((orch_id, task_id)) = self.pid_to_task.get(&pid) {
            if let Some(orch) = self.orchestrations.get_mut(orch_id) {
                let entry = orch.output.entry(task_id.clone()).or_default();
                append_with_cap(
                    entry,
                    text,
                    self.max_output_chars,
                    &mut orch.truncated_outputs,
                );
                orch.output_chars_stored = orch.output.values().map(|value| value.len()).sum();
            }
        }
    }

    /// Mark a task as completed (process finished normally).
    pub fn mark_completed(&mut self, pid: u64) {
        if let Some((orch_id, task_id)) = self.pid_to_task.remove(&pid) {
            if let Some(orch) = self.orchestrations.get_mut(&orch_id) {
                orch.status.insert(task_id, TaskStatus::Completed);
            }
        }
    }

    /// Mark a task as failed (process error).
    pub fn mark_failed(&mut self, pid: u64, error: &str) {
        if let Some((orch_id, task_id)) = self.pid_to_task.remove(&pid) {
            if let Some(orch) = self.orchestrations.get_mut(&orch_id) {
                orch.status.insert(
                    task_id,
                    TaskStatus::Failed {
                        error: error.to_string(),
                    },
                );
            }
        }
    }

    /// Advance all orchestrations: propagate failures, collect tasks ready
    /// to spawn. Also returns PIDs of running tasks that must be killed
    /// (fail-fast policy).
    pub fn advance(&mut self) -> (Vec<SpawnRequest>, Vec<u64>) {
        let mut all_requests = Vec::new();
        let mut kill_pids = Vec::new();

        let orch_ids: Vec<u64> = self.orchestrations.keys().copied().collect();
        for orch_id in orch_ids {
            let Some(orch) = self.orchestrations.get_mut(&orch_id) else {
                continue;
            };

            let has_failure = orch
                .status
                .values()
                .any(|status| matches!(status, TaskStatus::Failed { .. }));

            if has_failure && orch.failure_policy == FailurePolicy::FailFast {
                kill_pids.extend(orch.running_pids());
                for status in orch.status.values_mut() {
                    if matches!(status, TaskStatus::Pending | TaskStatus::Running { .. }) {
                        *status = TaskStatus::Skipped;
                    }
                }
                self.pid_to_task
                    .retain(|_, (existing_orch_id, _)| *existing_orch_id != orch_id);
                continue;
            }

            let topo = orch.topo_order.clone();
            for task_id in &topo {
                if !matches!(orch.status.get(task_id), Some(TaskStatus::Pending)) {
                    continue;
                }

                let Some(task) = orch.tasks.get(task_id) else {
                    continue;
                };
                let any_dep_failed = task.deps.iter().any(|dep| {
                    matches!(
                        orch.status.get(dep),
                        Some(TaskStatus::Failed { .. }) | Some(TaskStatus::Skipped)
                    )
                });
                if any_dep_failed {
                    orch.status.insert(task_id.clone(), TaskStatus::Skipped);
                }
            }

            let owner_id = orch.owner_id;
            for task_id in &topo {
                if !matches!(orch.status.get(task_id), Some(TaskStatus::Pending)) {
                    continue;
                }

                let Some(task) = orch.tasks.get(task_id) else {
                    continue;
                };
                let all_deps_done = task
                    .deps
                    .iter()
                    .all(|dep| matches!(orch.status.get(dep), Some(TaskStatus::Completed)));
                if !all_deps_done {
                    continue;
                }

                all_requests.push(SpawnRequest {
                    orch_id,
                    task_id: task_id.clone(),
                    prompt: build_task_prompt(task, &orch.output),
                    workload: workload_from_label_or_default(task.workload.as_deref()),
                    owner_id,
                    context_policy: task.resolved_context_policy(),
                });
            }
        }

        (all_requests, kill_pids)
    }

    /// Look up an orchestration by id.
    pub fn get(&self, orch_id: u64) -> Option<&Orchestration> {
        self.orchestrations.get(&orch_id)
    }

    pub fn task_binding(&self, pid: u64) -> Option<(u64, String)> {
        self.pid_to_task.get(&pid).cloned()
    }

    pub fn active_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .orchestrations
            .iter()
            .filter_map(|(orch_id, orch)| (!orch.is_finished()).then_some(*orch_id))
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Format a human-readable status report for one orchestration.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn format_status(&self, orch_id: u64) -> Option<String> {
        let orch = self.orchestrations.get(&orch_id)?;
        let (pending, running, completed, failed, skipped) = orch.counts();
        let total = orch.tasks.len();
        let elapsed = orch.created_at.elapsed().as_secs_f64();
        let finished = orch.is_finished();

        let mut lines = vec![format!(
            "orchestration_id={} total={} completed={} running={} pending={} failed={} skipped={} finished={} elapsed_secs={:.2} policy={:?}",
            orch_id,
            total,
            completed,
            running,
            pending,
            failed,
            skipped,
            finished,
            elapsed,
            orch.failure_policy,
        )];

        for task_id in &orch.topo_order {
            let Some(status) = orch.status.get(task_id) else {
                continue;
            };
            let detail = match status {
                TaskStatus::Running { pid } => format!(" pid={}", pid),
                TaskStatus::Failed { error } => format!(" error={}", error),
                _ => String::new(),
            };
            lines.push(format!(
                "  task={} status={}{}",
                task_id,
                status.label(),
                detail,
            ));
        }

        Some(lines.join("\n"))
    }
}
