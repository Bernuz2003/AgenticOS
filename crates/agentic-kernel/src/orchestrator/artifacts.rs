use super::*;

impl Orchestrator {
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

    pub fn running_output_for_task(
        &self,
        orch_id: u64,
        task_id: &str,
    ) -> Option<&RunningTaskOutput> {
        self.orchestrations
            .get(&orch_id)
            .and_then(|orch| orch.running_output.get(task_id))
    }
}

pub(super) fn refresh_output_metrics(orch: &mut Orchestration) {
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
