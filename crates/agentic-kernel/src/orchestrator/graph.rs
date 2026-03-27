use super::*;

impl Orchestrator {
    pub fn new() -> Self {
        Self {
            orchestrations: HashMap::new(),
            next_id: 1,
            pid_to_task: HashMap::new(),
            max_output_chars: crate::config::kernel_config().orchestrator.max_output_chars,
        }
    }

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

    pub fn get(&self, orch_id: u64) -> Option<&Orchestration> {
        self.orchestrations.get(&orch_id)
    }

    pub fn task_binding(&self, pid: u64) -> Option<(u64, String, u32)> {
        self.pid_to_task.get(&pid).cloned()
    }

    pub fn all_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.orchestrations.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

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

pub(super) fn build_spawn_request(
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

pub(super) fn allocate_attempt(orch: &mut Orchestration, task_id: &str) -> u32 {
    let next = orch.next_attempt.entry(task_id.to_string()).or_insert(1);
    let attempt = *next;
    *next = next.saturating_add(1);
    attempt
}
