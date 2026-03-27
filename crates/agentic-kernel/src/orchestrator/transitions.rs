use super::*;

impl Orchestrator {
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

    pub fn is_orchestrated(&self, pid: u64) -> bool {
        self.pid_to_task.contains_key(&pid)
    }

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

    pub fn remove(&mut self, orch_id: u64) -> bool {
        self.pid_to_task
            .retain(|_, (existing_orch_id, _, _)| *existing_orch_id != orch_id);
        self.orchestrations.remove(&orch_id).is_some()
    }
}
