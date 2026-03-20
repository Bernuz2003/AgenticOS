//! Agent orchestration — DAG-based multi-task workflow engine.
//!
//! Provides kernel-side logic for structured multi-agent workflows.
//! An orchestration is a directed acyclic graph (DAG) of tasks where each
//! task is an LLM prompt execution. Dependencies define data-flow: task
//! artifacts from completed upstream nodes are injected as context into
//! successor tasks.

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
    FailurePolicy, Orchestration, Orchestrator, RetryPlan, RunningTaskOutput, SpawnRequest,
    TaskArtifact, TaskAttemptFinalization, TaskGraphDef, TaskInputArtifact, TaskNodeDef,
    TaskPidBinding, TaskStatus,
};
use validation::validate_and_sort;

#[derive(Debug, Default)]
pub(crate) struct StopOrchestrationPlan {
    pub(crate) kill_pids: Vec<u64>,
    pub(crate) finalized_attempts: Vec<TaskAttemptFinalization>,
}

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

        let mut orchestration =
            Orchestration::new(owner_id, graph.failure_policy, tasks, topo_order, status);
        let root_ids = orchestration
            .topo_order
            .iter()
            .filter(|task_id| {
                orchestration
                    .tasks
                    .get(task_id.as_str())
                    .map(|task| task.deps.is_empty())
                    .unwrap_or(false)
            })
            .cloned()
            .collect::<Vec<_>>();

        let mut spawn_requests = Vec::new();
        for task_id in root_ids {
            let task = orchestration
                .tasks
                .get(task_id.as_str())
                .expect("task must exist")
                .clone();
            spawn_requests.push(build_spawn_request(
                orch_id,
                owner_id,
                &mut orchestration,
                &task_id,
                &task,
                Vec::new(),
            )?);
        }

        self.orchestrations.insert(orch_id, orchestration);
        Ok((orch_id, spawn_requests))
    }

    /// Register a spawned PID for a task attempt.
    pub fn register_pid(&mut self, pid: u64, orch_id: u64, task_id: &str, attempt: u32) {
        self.pid_to_task
            .insert(pid, (orch_id, task_id.to_string(), attempt));
        if let Some(orch) = self.orchestrations.get_mut(&orch_id) {
            orch.status
                .insert(task_id.to_string(), TaskStatus::Running { pid, attempt });
            orch.running_output.insert(
                task_id.to_string(),
                RunningTaskOutput {
                    attempt,
                    text: String::new(),
                    truncated: false,
                },
            );
            refresh_output_metrics(orch);
        }
    }

    pub fn task_binding_for_pid(&self, pid: u64) -> Option<TaskPidBinding> {
        let (orch_id, task_id, attempt) = self.pid_to_task.get(&pid)?.clone();
        Some(TaskPidBinding {
            orch_id,
            task_id,
            attempt,
        })
    }

    /// Mark a task whose spawn failed (routing/admission/spawn).
    pub fn mark_spawn_failed(
        &mut self,
        orch_id: u64,
        task_id: &str,
        attempt: u32,
        error: &str,
    ) -> Option<TaskAttemptFinalization> {
        let orch = self.orchestrations.get_mut(&orch_id)?;
        orch.running_output.remove(task_id);
        orch.latest_artifacts.remove(task_id);
        orch.status.insert(
            task_id.to_string(),
            TaskStatus::Failed {
                error: error.to_string(),
                attempt,
            },
        );
        refresh_output_metrics(orch);
        Some(TaskAttemptFinalization {
            orch_id,
            task_id: task_id.to_string(),
            attempt,
            status: "failed".to_string(),
            error: Some(error.to_string()),
            termination_reason: Some("spawn_failed".to_string()),
            output_text: String::new(),
            truncated: false,
        })
    }

    pub fn record_completed_artifact(
        &mut self,
        orch_id: u64,
        task_id: &str,
        artifact: TaskArtifact,
    ) {
        if let Some(orch) = self.orchestrations.get_mut(&orch_id) {
            orch.latest_artifacts.insert(task_id.to_string(), artifact);
            refresh_output_metrics(orch);
        }
    }

    /// Check if a PID belongs to an orchestration.
    pub fn is_orchestrated(&self, pid: u64) -> bool {
        self.pid_to_task.contains_key(&pid)
    }

    /// Append generated text to the live output buffer of a task attempt.
    pub fn append_output(&mut self, pid: u64, text: &str) {
        if let Some((orch_id, task_id, attempt)) = self.pid_to_task.get(&pid) {
            if let Some(orch) = self.orchestrations.get_mut(orch_id) {
                let entry = orch
                    .running_output
                    .entry(task_id.clone())
                    .or_insert_with(|| RunningTaskOutput {
                        attempt: *attempt,
                        text: String::new(),
                        truncated: false,
                    });
                let previous_truncations = orch.truncated_outputs;
                append_with_cap(
                    &mut entry.text,
                    text,
                    self.max_output_chars,
                    &mut orch.truncated_outputs,
                );
                if orch.truncated_outputs > previous_truncations {
                    entry.truncated = true;
                }
                refresh_output_metrics(orch);
            }
        }
    }

    /// Mark a task as completed and return the finalized attempt payload.
    pub fn mark_completed(
        &mut self,
        pid: u64,
        termination_reason: Option<&str>,
    ) -> Option<TaskAttemptFinalization> {
        let (orch_id, task_id, attempt) = self.pid_to_task.remove(&pid)?;
        let orch = self.orchestrations.get_mut(&orch_id)?;
        let output = orch.running_output.remove(&task_id);
        orch.status
            .insert(task_id.clone(), TaskStatus::Completed { attempt });
        refresh_output_metrics(orch);
        Some(TaskAttemptFinalization {
            orch_id,
            task_id,
            attempt,
            status: "completed".to_string(),
            error: None,
            termination_reason: termination_reason.map(ToString::to_string),
            output_text: output
                .as_ref()
                .map(|item| item.text.clone())
                .unwrap_or_default(),
            truncated: output.as_ref().map(|item| item.truncated).unwrap_or(false),
        })
    }

    /// Mark a task as failed and return the finalized attempt payload.
    pub fn mark_failed(
        &mut self,
        pid: u64,
        error: &str,
        termination_reason: Option<&str>,
    ) -> Option<TaskAttemptFinalization> {
        let (orch_id, task_id, attempt) = self.pid_to_task.remove(&pid)?;
        let orch = self.orchestrations.get_mut(&orch_id)?;
        let output = orch.running_output.remove(&task_id);
        orch.status.insert(
            task_id.clone(),
            TaskStatus::Failed {
                error: error.to_string(),
                attempt,
            },
        );
        refresh_output_metrics(orch);
        Some(TaskAttemptFinalization {
            orch_id,
            task_id,
            attempt,
            status: "failed".to_string(),
            error: Some(error.to_string()),
            termination_reason: termination_reason
                .map(ToString::to_string)
                .or_else(|| Some("worker_error".to_string())),
            output_text: output
                .as_ref()
                .map(|item| item.text.clone())
                .unwrap_or_default(),
            truncated: output.as_ref().map(|item| item.truncated).unwrap_or(false),
        })
    }

    pub fn retry_task(
        &mut self,
        orch_id: u64,
        task_id: &str,
    ) -> Result<RetryPlan, OrchestratorError> {
        let Some(orch) = self.orchestrations.get_mut(&orch_id) else {
            return Err(OrchestratorError::RetryTaskNotFound {
                orchestration_id: orch_id,
                task: task_id.to_string(),
            });
        };
        if !orch.tasks.contains_key(task_id) {
            return Err(OrchestratorError::RetryTaskNotFound {
                orchestration_id: orch_id,
                task: task_id.to_string(),
            });
        }

        let reset_tasks = descendant_tasks(orch, task_id);
        if let Some(running_task) = reset_tasks.iter().find(|candidate| {
            matches!(
                orch.status.get(candidate.as_str()),
                Some(TaskStatus::Running { .. })
            )
        }) {
            return Err(OrchestratorError::RetryTaskBusy {
                orchestration_id: orch_id,
                task: running_task.clone(),
            });
        }

        if orch.failure_policy == FailurePolicy::FailFast {
            if let Some(blocking_task) = orch
                .status
                .iter()
                .find(|(candidate, status)| {
                    !reset_tasks.contains(candidate) && matches!(status, TaskStatus::Failed { .. })
                })
                .map(|(candidate, _)| candidate.clone())
            {
                return Err(OrchestratorError::RetryBlockedByFailure {
                    orchestration_id: orch_id,
                    task: task_id.to_string(),
                    blocking_task,
                });
            }
        }

        for candidate in &reset_tasks {
            orch.status.insert(candidate.clone(), TaskStatus::Pending);
            orch.running_output.remove(candidate);
            orch.latest_artifacts.remove(candidate);
        }
        refresh_output_metrics(orch);

        Ok(RetryPlan { reset_tasks })
    }

    /// Advance all orchestrations: propagate failures, collect tasks ready
    /// to spawn. Also returns PIDs of running tasks that must be killed
    /// (fail-fast policy).
    pub fn advance(&mut self) -> (Vec<SpawnRequest>, Vec<u64>) {
        let orch_ids = self.orchestrations.keys().copied().collect::<Vec<_>>();
        self.advance_ids(&orch_ids)
    }

    pub fn advance_one(&mut self, orch_id: u64) -> (Vec<SpawnRequest>, Vec<u64>) {
        self.advance_ids(&[orch_id])
    }

    fn advance_ids(&mut self, orch_ids: &[u64]) -> (Vec<SpawnRequest>, Vec<u64>) {
        let mut all_requests = Vec::new();
        let mut kill_pids = Vec::new();

        for &orch_id in orch_ids {
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
                    .retain(|_, (existing_orch_id, _, _)| *existing_orch_id != orch_id);
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

                let Some(task) = orch.tasks.get(task_id).cloned() else {
                    continue;
                };
                let all_deps_done = task
                    .deps
                    .iter()
                    .all(|dep| matches!(orch.status.get(dep), Some(TaskStatus::Completed { .. })));
                if !all_deps_done {
                    continue;
                }

                let input_artifacts = task
                    .deps
                    .iter()
                    .filter_map(|dep| orch.latest_artifacts.get(dep))
                    .map(|artifact| TaskInputArtifact {
                        artifact_id: artifact.artifact_id.clone(),
                        producer_task_id: artifact.producer_task_id.clone(),
                        producer_attempt: artifact.producer_attempt,
                        mime_type: artifact.mime_type.clone(),
                        content_text: artifact.content_text.clone(),
                    })
                    .collect::<Vec<_>>();
                all_requests.push(
                    build_spawn_request(orch_id, owner_id, orch, task_id, &task, input_artifacts)
                        .expect("orchestration task permissions must be validated at registration"),
                );
            }
        }

        (all_requests, kill_pids)
    }

    /// Look up an orchestration by id.
    pub fn get(&self, orch_id: u64) -> Option<&Orchestration> {
        self.orchestrations.get(&orch_id)
    }

    pub fn task_binding(&self, pid: u64) -> Option<(u64, String, u32)> {
        self.pid_to_task.get(&pid).cloned()
    }

    pub fn running_output_for_task(
        &self,
        orch_id: u64,
        task_id: &str,
    ) -> Option<&RunningTaskOutput> {
        self.orchestrations
            .get(&orch_id)
            .and_then(|orch| orch.running_output.get(task_id))
    }

    pub fn all_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.orchestrations.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    pub(crate) fn stop(&mut self, orch_id: u64) -> Option<StopOrchestrationPlan> {
        let orch = self.orchestrations.get_mut(&orch_id)?;
        let mut plan = StopOrchestrationPlan::default();
        let task_ids = orch.topo_order.clone();

        for task_id in task_ids {
            match orch.status.get(&task_id).cloned() {
                Some(TaskStatus::Running { pid, attempt }) => {
                    let output = orch.running_output.remove(&task_id);
                    orch.status.insert(task_id.clone(), TaskStatus::Skipped);
                    plan.kill_pids.push(pid);
                    plan.finalized_attempts.push(TaskAttemptFinalization {
                        orch_id,
                        task_id,
                        attempt,
                        status: "skipped".to_string(),
                        error: Some("orchestration_stopped".to_string()),
                        termination_reason: Some("orchestration_stopped".to_string()),
                        output_text: output
                            .as_ref()
                            .map(|item| item.text.clone())
                            .unwrap_or_default(),
                        truncated: output.as_ref().map(|item| item.truncated).unwrap_or(false),
                    });
                }
                Some(TaskStatus::Pending) => {
                    orch.status.insert(task_id, TaskStatus::Skipped);
                }
                _ => {}
            }
        }

        self.pid_to_task
            .retain(|_, (existing_orch_id, _, _)| *existing_orch_id != orch_id);
        refresh_output_metrics(orch);
        Some(plan)
    }

    pub fn remove(&mut self, orch_id: u64) -> bool {
        self.pid_to_task
            .retain(|_, (existing_orch_id, _, _)| *existing_orch_id != orch_id);
        self.orchestrations.remove(&orch_id).is_some()
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
                TaskStatus::Running { pid, attempt } => {
                    format!(" pid={} attempt={}", pid, attempt)
                }
                TaskStatus::Completed { attempt } => format!(" attempt={}", attempt),
                TaskStatus::Failed { error, attempt } => {
                    format!(" attempt={} error={}", attempt, error)
                }
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

fn build_spawn_request(
    orch_id: u64,
    owner_id: usize,
    orchestration: &mut Orchestration,
    task_id: &str,
    task: &TaskNodeDef,
    input_artifacts: Vec<TaskInputArtifact>,
) -> Result<SpawnRequest, OrchestratorError> {
    let attempt = allocate_attempt(orchestration, task_id);
    Ok(SpawnRequest {
        orch_id,
        task_id: task_id.to_string(),
        attempt,
        prompt: build_task_prompt(task, &input_artifacts),
        input_artifacts,
        workload: workload_from_label_or_default(task.workload.as_deref()),
        required_backend_class: task.backend_class,
        owner_id,
        context_policy: task.resolved_context_policy(),
        permission_overrides: task.permission_overrides()?,
    })
}

fn allocate_attempt(orch: &mut Orchestration, task_id: &str) -> u32 {
    let next = orch.next_attempt.entry(task_id.to_string()).or_insert(1);
    let attempt = *next;
    *next = next.saturating_add(1);
    attempt
}

fn refresh_output_metrics(orch: &mut Orchestration) {
    orch.output_chars_stored = orch
        .latest_artifacts
        .values()
        .map(|artifact| artifact.content_text.len())
        .sum::<usize>()
        + orch
            .running_output
            .values()
            .map(|output| output.text.len())
            .sum::<usize>();
}

fn descendant_tasks(orch: &Orchestration, root_task: &str) -> Vec<String> {
    let mut selected = vec![root_task.to_string()];
    let mut changed = true;
    while changed {
        changed = false;
        for task_id in &orch.topo_order {
            if selected.contains(task_id) {
                continue;
            }
            let Some(task) = orch.tasks.get(task_id) else {
                continue;
            };
            if task.deps.iter().any(|dep| selected.contains(dep)) {
                selected.push(task_id.clone());
                changed = true;
            }
        }
    }
    selected
}
