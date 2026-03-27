use super::*;

#[derive(Debug, Default)]
pub(crate) struct StopOrchestrationPlan {
    pub(crate) kill_pids: Vec<u64>,
    pub(crate) finalized_attempts: Vec<TaskAttemptFinalization>,
}

impl Orchestrator {
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
