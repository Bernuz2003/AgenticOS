//! Agent orchestration — DAG-based multi-task workflow engine.
//!
//! Provides kernel-side logic for structured multi-agent workflows.
//! An orchestration is a directed acyclic graph (DAG) of tasks where each
//! task is an LLM prompt execution.  Dependencies define data-flow: the
//! output of completed tasks is injected as context into successor tasks.
//!
//! This module does **not** spawn LLM processes — it produces
//! [`SpawnRequest`] values consumed by the runtime loop to call
//! `LLMEngine::spawn_process`.

use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use crate::model_catalog::WorkloadClass;

// ── JSON schema ─────────────────────────────────────────────────────────

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
    /// Unique identifier for this task within the graph.
    pub id: String,
    /// LLM prompt to execute.
    pub prompt: String,
    /// Optional workload hint: `"fast"`, `"code"`, `"reasoning"`, `"general"`.
    #[serde(default)]
    pub workload: Option<String>,
    /// IDs of tasks this one depends on (must complete first).
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

// ── Runtime types ───────────────────────────────────────────────────────

/// Runtime status of a single task within an orchestration.
#[derive(Debug, Clone)]
pub enum TaskStatus {
    /// Waiting for dependencies to complete.
    Pending,
    /// Spawned as LLM process; PID assigned.
    Running { pid: u64 },
    /// Finished successfully; output captured.
    Completed,
    /// Process errored or task cancelled.
    Failed { error: String },
    /// Skipped because a dependency failed.
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
    /// Task definitions keyed by task id.
    pub tasks: HashMap<String, TaskNodeDef>,
    /// Deterministic topological order (Kahn's algorithm, tie-break alphabetical).
    pub topo_order: Vec<String>,
    /// Current status per task id.
    pub status: HashMap<String, TaskStatus>,
    /// Accumulated LLM output per task id.
    pub output: HashMap<String, String>,
    pub created_at: Instant,
}

impl Orchestration {
    /// Returns `true` when every task has reached a terminal state.
    pub fn is_finished(&self) -> bool {
        self.status.values().all(|s| s.is_terminal())
    }

    /// `(pending, running, completed, failed, skipped)`
    pub fn counts(&self) -> (usize, usize, usize, usize, usize) {
        let (mut p, mut r, mut c, mut f, mut s) = (0, 0, 0, 0, 0);
        for st in self.status.values() {
            match st {
                TaskStatus::Pending => p += 1,
                TaskStatus::Running { .. } => r += 1,
                TaskStatus::Completed => c += 1,
                TaskStatus::Failed { .. } => f += 1,
                TaskStatus::Skipped => s += 1,
            }
        }
        (p, r, c, f, s)
    }

    /// Collect PIDs of currently running tasks.
    fn running_pids(&self) -> Vec<u64> {
        self.status
            .values()
            .filter_map(|s| {
                if let TaskStatus::Running { pid } = s {
                    Some(*pid)
                } else {
                    None
                }
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

// ── Orchestrator ────────────────────────────────────────────────────────

/// Manages all active orchestrations.
pub struct Orchestrator {
    orchestrations: HashMap<u64, Orchestration>,
    next_id: u64,
    /// Maps active PID → (orchestration_id, task_id).
    pid_to_task: HashMap<u64, (u64, String)>,
}

impl Orchestrator {
    pub fn new() -> Self {
        Self {
            orchestrations: HashMap::new(),
            next_id: 1,
            pid_to_task: HashMap::new(),
        }
    }

    // ── Graph registration ──────────────────────────────────────────────

    /// Validate and register a new task graph.
    /// Returns `(orchestration_id, initial_spawn_requests)` for root tasks.
    pub fn register(
        &mut self,
        graph: TaskGraphDef,
        owner_id: usize,
    ) -> Result<(u64, Vec<SpawnRequest>), String> {
        // ── Validation ──────────────────────────────────────────────
        if graph.tasks.is_empty() {
            return Err("task graph is empty".to_string());
        }

        let mut seen = HashSet::new();
        for task in &graph.tasks {
            if !seen.insert(&task.id) {
                return Err(format!("duplicate task id '{}'", task.id));
            }
        }

        let task_ids: HashSet<&str> = graph.tasks.iter().map(|t| t.id.as_str()).collect();
        for task in &graph.tasks {
            if task.deps.contains(&task.id) {
                return Err(format!("task '{}' depends on itself", task.id));
            }
            for dep in &task.deps {
                if !task_ids.contains(dep.as_str()) {
                    return Err(format!(
                        "task '{}' depends on unknown task '{}'",
                        task.id, dep
                    ));
                }
            }
        }

        let topo_order = topological_sort(&graph.tasks)?;

        // ── Build orchestration ─────────────────────────────────────
        let orch_id = self.next_id;
        self.next_id += 1;

        let tasks: HashMap<String, TaskNodeDef> = graph
            .tasks
            .iter()
            .map(|t| (t.id.clone(), t.clone()))
            .collect();
        let status: HashMap<String, TaskStatus> = graph
            .tasks
            .iter()
            .map(|t| (t.id.clone(), TaskStatus::Pending))
            .collect();

        let orch = Orchestration {
            owner_id,
            failure_policy: graph.failure_policy,
            tasks,
            topo_order,
            status,
            output: HashMap::new(),
            created_at: Instant::now(),
        };

        self.orchestrations.insert(orch_id, orch);

        // ── Collect root spawn requests (deps == []) ────────────────
        let orch = self.orchestrations.get(&orch_id).unwrap();
        let mut spawn_requests = Vec::new();
        for task_id in &orch.topo_order {
            let task = &orch.tasks[task_id];
            if task.deps.is_empty() {
                spawn_requests.push(SpawnRequest {
                    orch_id,
                    task_id: task_id.clone(),
                    prompt: task.prompt.clone(),
                    workload: parse_workload_str(task.workload.as_deref()),
                    owner_id,
                });
            }
        }

        Ok((orch_id, spawn_requests))
    }

    // ── PID tracking ────────────────────────────────────────────────────

    /// Register a spawned PID for a task.
    pub fn register_pid(&mut self, pid: u64, orch_id: u64, task_id: &str) {
        self.pid_to_task
            .insert(pid, (orch_id, task_id.to_string()));
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

    // ── Runtime tracking ────────────────────────────────────────────────

    /// Check if a PID belongs to an orchestration.
    pub fn is_orchestrated(&self, pid: u64) -> bool {
        self.pid_to_task.contains_key(&pid)
    }

    /// Append generated text to a task's output buffer.
    pub fn append_output(&mut self, pid: u64, text: &str) {
        if let Some((orch_id, task_id)) = self.pid_to_task.get(&pid) {
            if let Some(orch) = self.orchestrations.get_mut(orch_id) {
                orch.output
                    .entry(task_id.clone())
                    .or_default()
                    .push_str(text);
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

    // ── Advance logic ───────────────────────────────────────────────────

    /// Advance all orchestrations: propagate failures, collect tasks ready
    /// to spawn.  Also returns PIDs of running tasks that must be killed
    /// (fail-fast policy).
    pub fn advance(&mut self) -> (Vec<SpawnRequest>, Vec<u64>) {
        let mut all_requests = Vec::new();
        let mut kill_pids = Vec::new();

        let orch_ids: Vec<u64> = self.orchestrations.keys().copied().collect();

        for orch_id in orch_ids {
            let orch = self.orchestrations.get_mut(&orch_id).unwrap();

            let has_failure = orch
                .status
                .values()
                .any(|s| matches!(s, TaskStatus::Failed { .. }));

            // ── Fail-fast: abort everything ─────────────────────────
            if has_failure && orch.failure_policy == FailurePolicy::FailFast {
                kill_pids.extend(orch.running_pids());
                for status in orch.status.values_mut() {
                    if matches!(status, TaskStatus::Pending | TaskStatus::Running { .. }) {
                        *status = TaskStatus::Skipped;
                    }
                }
                // Clean up pid_to_task entries for this orchestration.
                self.pid_to_task.retain(|_, (oid, _)| *oid != orch_id);
                continue;
            }

            // ── Best-effort: skip tasks with failed/skipped deps ────
            // Iterate in topo order so skips propagate transitively.
            let topo = orch.topo_order.clone();
            for task_id in &topo {
                if !matches!(orch.status[task_id], TaskStatus::Pending) {
                    continue;
                }
                let deps = &orch.tasks[task_id].deps;
                let any_dep_failed = deps.iter().any(|dep| {
                    matches!(
                        orch.status.get(dep),
                        Some(TaskStatus::Failed { .. }) | Some(TaskStatus::Skipped)
                    )
                });
                if any_dep_failed {
                    orch.status.insert(task_id.clone(), TaskStatus::Skipped);
                }
            }

            // ── Collect ready tasks (all deps completed) ────────────
            let owner_id = orch.owner_id;
            for task_id in &topo {
                if !matches!(orch.status[task_id], TaskStatus::Pending) {
                    continue;
                }
                let task = &orch.tasks[task_id];
                let all_deps_done = task
                    .deps
                    .iter()
                    .all(|dep| matches!(orch.status[dep], TaskStatus::Completed));
                if !all_deps_done {
                    continue;
                }

                let prompt = build_task_prompt(task, &orch.output);
                let workload = parse_workload_str(task.workload.as_deref());

                all_requests.push(SpawnRequest {
                    orch_id,
                    task_id: task_id.clone(),
                    prompt,
                    workload,
                    owner_id,
                });
            }
        }

        (all_requests, kill_pids)
    }

    // ── Query ───────────────────────────────────────────────────────────

    /// Look up an orchestration by id.
    pub fn get(&self, orch_id: u64) -> Option<&Orchestration> {
        self.orchestrations.get(&orch_id)
    }

    /// Format a human-readable status report for one orchestration.
    pub fn format_status(&self, orch_id: u64) -> Option<String> {
        let orch = self.orchestrations.get(&orch_id)?;
        let (pending, running, completed, failed, skipped) = orch.counts();
        let total = orch.tasks.len();
        let elapsed = orch.created_at.elapsed().as_secs_f64();
        let finished = orch.is_finished();

        let mut lines = vec![format!(
            "orchestration_id={} total={} completed={} running={} pending={} failed={} skipped={} finished={} elapsed_secs={:.2} policy={:?}",
            orch_id, total, completed, running, pending, failed, skipped, finished, elapsed, orch.failure_policy
        )];

        for task_id in &orch.topo_order {
            let status = &orch.status[task_id];
            let detail = match status {
                TaskStatus::Running { pid } => format!(" pid={}", pid),
                TaskStatus::Failed { error } => format!(" error={}", error),
                _ => String::new(),
            };
            lines.push(format!(
                "  task={} status={}{}",
                task_id,
                status.label(),
                detail
            ));
        }

        Some(lines.join("\n"))
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn parse_workload_str(s: Option<&str>) -> WorkloadClass {
    match s.map(|v| v.to_lowercase()).as_deref() {
        Some("fast") => WorkloadClass::Fast,
        Some("code") => WorkloadClass::Code,
        Some("reasoning") => WorkloadClass::Reasoning,
        _ => WorkloadClass::General,
    }
}

/// Build the prompt for a dependent task, injecting output from its
/// completed dependencies as context.
fn build_task_prompt(task: &TaskNodeDef, outputs: &HashMap<String, String>) -> String {
    let mut context_parts = Vec::new();
    for dep in &task.deps {
        if let Some(output) = outputs.get(dep) {
            if !output.is_empty() {
                context_parts.push(format!("[Output from task \"{}\"]:\n{}", dep, output));
            }
        }
    }
    if context_parts.is_empty() {
        task.prompt.clone()
    } else {
        format!(
            "{}\n\n[Your task]:\n{}",
            context_parts.join("\n\n"),
            task.prompt
        )
    }
}

/// Topological sort (Kahn's algorithm).  Tie-breaks alphabetically for
/// deterministic ordering.  Returns `Err` if the graph contains a cycle.
fn topological_sort(tasks: &[TaskNodeDef]) -> Result<Vec<String>, String> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

    for task in tasks {
        in_degree.entry(task.id.as_str()).or_insert(0);
        adj.entry(task.id.as_str()).or_default();
        for dep in &task.deps {
            adj.entry(dep.as_str()).or_default().push(task.id.as_str());
            *in_degree.entry(task.id.as_str()).or_insert(0) += 1;
        }
    }

    // Seed queue with zero-indegree nodes (sorted for determinism).
    let mut roots: Vec<&str> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();
    roots.sort();

    let mut queue: VecDeque<&str> = roots.into_iter().collect();
    let mut result = Vec::new();

    while let Some(node) = queue.pop_front() {
        result.push(node.to_string());
        if let Some(children) = adj.get(node) {
            let mut ready = Vec::new();
            for &child in children {
                let deg = in_degree.get_mut(child).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    ready.push(child);
                }
            }
            ready.sort();
            queue.extend(ready);
        }
    }

    if result.len() != tasks.len() {
        return Err("task graph contains a cycle".to_string());
    }

    Ok(result)
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_linear_graph() -> TaskGraphDef {
        TaskGraphDef {
            tasks: vec![
                TaskNodeDef {
                    id: "A".into(),
                    prompt: "Task A".into(),
                    workload: None,
                    deps: vec![],
                },
                TaskNodeDef {
                    id: "B".into(),
                    prompt: "Task B".into(),
                    workload: Some("code".into()),
                    deps: vec!["A".into()],
                },
                TaskNodeDef {
                    id: "C".into(),
                    prompt: "Task C".into(),
                    workload: None,
                    deps: vec!["B".into()],
                },
            ],
            failure_policy: FailurePolicy::FailFast,
        }
    }

    fn make_parallel_graph() -> TaskGraphDef {
        TaskGraphDef {
            tasks: vec![
                TaskNodeDef {
                    id: "A".into(),
                    prompt: "Task A".into(),
                    workload: None,
                    deps: vec![],
                },
                TaskNodeDef {
                    id: "B".into(),
                    prompt: "Task B".into(),
                    workload: None,
                    deps: vec!["A".into()],
                },
                TaskNodeDef {
                    id: "C".into(),
                    prompt: "Task C".into(),
                    workload: None,
                    deps: vec!["A".into()],
                },
                TaskNodeDef {
                    id: "D".into(),
                    prompt: "Task D".into(),
                    workload: None,
                    deps: vec!["B".into(), "C".into()],
                },
            ],
            failure_policy: FailurePolicy::BestEffort,
        }
    }

    // ── Registration ────────────────────────────────────────────────────

    #[test]
    fn linear_graph_registers_and_spawns_root() {
        let mut orch = Orchestrator::new();
        let (id, spawns) = orch.register(make_linear_graph(), 1).expect("register");
        assert_eq!(id, 1);
        assert_eq!(spawns.len(), 1);
        assert_eq!(spawns[0].task_id, "A");
        assert_eq!(spawns[0].prompt, "Task A");
    }

    #[test]
    fn parallel_graph_spawns_single_root() {
        let mut orch = Orchestrator::new();
        let (_, spawns) = orch.register(make_parallel_graph(), 1).unwrap();
        assert_eq!(spawns.len(), 1);
        assert_eq!(spawns[0].task_id, "A");
    }

    // ── Linear advancement ──────────────────────────────────────────────

    #[test]
    fn linear_graph_advances_step_by_step() {
        let mut orch = Orchestrator::new();
        let (id, spawns) = orch.register(make_linear_graph(), 1).unwrap();

        // Spawn and complete A.
        let pid_a = 100;
        orch.register_pid(pid_a, id, &spawns[0].task_id);
        orch.append_output(pid_a, "result of A");
        orch.mark_completed(pid_a);

        let (ready, kills) = orch.advance();
        assert!(kills.is_empty());
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].task_id, "B");
        assert!(ready[0].prompt.contains("result of A"));
        assert!(ready[0].prompt.contains("Task B"));

        // Spawn and complete B.
        let pid_b = 101;
        orch.register_pid(pid_b, id, &ready[0].task_id);
        orch.append_output(pid_b, "result of B");
        orch.mark_completed(pid_b);

        let (ready, _) = orch.advance();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].task_id, "C");
        assert!(ready[0].prompt.contains("result of B"));

        // Complete C.
        let pid_c = 102;
        orch.register_pid(pid_c, id, &ready[0].task_id);
        orch.mark_completed(pid_c);

        let (ready, _) = orch.advance();
        assert!(ready.is_empty());
        assert!(orch.get(id).unwrap().is_finished());
    }

    // ── Parallel advancement ────────────────────────────────────────────

    #[test]
    fn parallel_graph_spawns_b_and_c_after_a() {
        let mut orch = Orchestrator::new();
        let (id, _spawns) = orch.register(make_parallel_graph(), 1).unwrap();

        let pid_a = 100;
        orch.register_pid(pid_a, id, "A");
        orch.append_output(pid_a, "A output");
        orch.mark_completed(pid_a);

        let (ready, _) = orch.advance();
        assert_eq!(ready.len(), 2);
        let ids: Vec<&str> = ready.iter().map(|r| r.task_id.as_str()).collect();
        assert!(ids.contains(&"B"));
        assert!(ids.contains(&"C"));

        // Complete B and C → D becomes ready.
        let pid_b = 101;
        let pid_c = 102;
        orch.register_pid(pid_b, id, "B");
        orch.register_pid(pid_c, id, "C");
        orch.append_output(pid_b, "B output");
        orch.append_output(pid_c, "C output");
        orch.mark_completed(pid_b);
        orch.mark_completed(pid_c);

        let (ready, _) = orch.advance();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].task_id, "D");
        assert!(ready[0].prompt.contains("B output"));
        assert!(ready[0].prompt.contains("C output"));
    }

    // ── Failure policies ────────────────────────────────────────────────

    #[test]
    fn fail_fast_skips_pending_on_failure() {
        let mut orch = Orchestrator::new();
        let (id, spawns) = orch.register(make_linear_graph(), 1).unwrap();

        let pid_a = 100;
        orch.register_pid(pid_a, id, &spawns[0].task_id);
        orch.mark_failed(pid_a, "process error");

        let (ready, kill_pids) = orch.advance();
        assert!(ready.is_empty());
        assert!(kill_pids.is_empty()); // A already failed, no running tasks left

        let o = orch.get(id).unwrap();
        assert!(matches!(o.status["B"], TaskStatus::Skipped));
        assert!(matches!(o.status["C"], TaskStatus::Skipped));
        assert!(o.is_finished());
    }

    #[test]
    fn fail_fast_kills_running_tasks() {
        let mut orch = Orchestrator::new();
        // Use a graph where B and C run in parallel after A.
        let graph = TaskGraphDef {
            tasks: vec![
                TaskNodeDef { id: "A".into(), prompt: "A".into(), workload: None, deps: vec![] },
                TaskNodeDef { id: "B".into(), prompt: "B".into(), workload: None, deps: vec!["A".into()] },
                TaskNodeDef { id: "C".into(), prompt: "C".into(), workload: None, deps: vec!["A".into()] },
            ],
            failure_policy: FailurePolicy::FailFast,
        };
        let (id, _) = orch.register(graph, 1).unwrap();

        // Complete A, advance to spawn B and C.
        let pid_a = 100;
        orch.register_pid(pid_a, id, "A");
        orch.mark_completed(pid_a);
        let (ready, _) = orch.advance();
        assert_eq!(ready.len(), 2);

        let pid_b = 101;
        let pid_c = 102;
        orch.register_pid(pid_b, id, "B");
        orch.register_pid(pid_c, id, "C");

        // B fails while C is still running.
        orch.mark_failed(pid_b, "oops");

        let (ready, kill_pids) = orch.advance();
        assert!(ready.is_empty());
        assert!(kill_pids.contains(&pid_c));

        let o = orch.get(id).unwrap();
        assert!(o.is_finished());
    }

    #[test]
    fn best_effort_skips_dependents_of_failed() {
        let mut orch = Orchestrator::new();
        let (id, _) = orch.register(make_parallel_graph(), 1).unwrap();

        let pid_a = 100;
        orch.register_pid(pid_a, id, "A");
        orch.append_output(pid_a, "A done");
        orch.mark_completed(pid_a);

        let (ready, _) = orch.advance();
        assert_eq!(ready.len(), 2);

        let pid_b = 101;
        let pid_c = 102;
        orch.register_pid(pid_b, id, "B");
        orch.register_pid(pid_c, id, "C");

        // B fails, C completes.
        orch.mark_failed(pid_b, "B error");
        orch.append_output(pid_c, "C output");
        orch.mark_completed(pid_c);

        let (ready, kill_pids) = orch.advance();
        assert!(kill_pids.is_empty()); // best_effort doesn't kill
        assert!(ready.is_empty()); // D depends on B (failed) → skipped

        let o = orch.get(id).unwrap();
        assert!(matches!(o.status["D"], TaskStatus::Skipped));
        assert!(o.is_finished());
    }

    // ── Validation ──────────────────────────────────────────────────────

    #[test]
    fn cyclic_graph_rejected() {
        let graph = TaskGraphDef {
            tasks: vec![
                TaskNodeDef { id: "A".into(), prompt: "A".into(), workload: None, deps: vec!["B".into()] },
                TaskNodeDef { id: "B".into(), prompt: "B".into(), workload: None, deps: vec!["A".into()] },
            ],
            failure_policy: FailurePolicy::FailFast,
        };
        let mut orch = Orchestrator::new();
        let err = orch.register(graph, 1).expect_err("cycle should fail");
        assert!(err.contains("cycle"));
    }

    #[test]
    fn empty_graph_rejected() {
        let graph = TaskGraphDef {
            tasks: vec![],
            failure_policy: FailurePolicy::FailFast,
        };
        let mut orch = Orchestrator::new();
        let err = orch.register(graph, 1).expect_err("empty should fail");
        assert!(err.contains("empty"));
    }

    #[test]
    fn duplicate_task_id_rejected() {
        let graph = TaskGraphDef {
            tasks: vec![
                TaskNodeDef { id: "A".into(), prompt: "A".into(), workload: None, deps: vec![] },
                TaskNodeDef { id: "A".into(), prompt: "A2".into(), workload: None, deps: vec![] },
            ],
            failure_policy: FailurePolicy::FailFast,
        };
        let mut orch = Orchestrator::new();
        let err = orch.register(graph, 1).expect_err("duplicate should fail");
        assert!(err.contains("duplicate"));
    }

    #[test]
    fn unknown_dependency_rejected() {
        let graph = TaskGraphDef {
            tasks: vec![TaskNodeDef {
                id: "A".into(),
                prompt: "A".into(),
                workload: None,
                deps: vec!["Z".into()],
            }],
            failure_policy: FailurePolicy::FailFast,
        };
        let mut orch = Orchestrator::new();
        let err = orch
            .register(graph, 1)
            .expect_err("unknown dep should fail");
        assert!(err.contains("unknown task"));
    }

    #[test]
    fn self_dependency_rejected() {
        let graph = TaskGraphDef {
            tasks: vec![TaskNodeDef {
                id: "A".into(),
                prompt: "A".into(),
                workload: None,
                deps: vec!["A".into()],
            }],
            failure_policy: FailurePolicy::FailFast,
        };
        let mut orch = Orchestrator::new();
        let err = orch.register(graph, 1).expect_err("self-dep should fail");
        assert!(err.contains("depends on itself"));
    }

    // ── Topological sort ────────────────────────────────────────────────

    #[test]
    fn topological_sort_deterministic() {
        let tasks = vec![
            TaskNodeDef { id: "C".into(), prompt: String::new(), workload: None, deps: vec!["A".into()] },
            TaskNodeDef { id: "A".into(), prompt: String::new(), workload: None, deps: vec![] },
            TaskNodeDef { id: "B".into(), prompt: String::new(), workload: None, deps: vec!["A".into()] },
        ];
        let order = topological_sort(&tasks).unwrap();
        assert_eq!(order, vec!["A", "B", "C"]);
    }

    // ── Status / format ─────────────────────────────────────────────────

    #[test]
    fn format_status_includes_all_info() {
        let mut orch = Orchestrator::new();
        let (id, _) = orch.register(make_linear_graph(), 1).unwrap();
        let status = orch.format_status(id).expect("status should exist");
        assert!(status.contains("orchestration_id=1"));
        assert!(status.contains("total=3"));
        assert!(status.contains("task=A"));
        assert!(status.contains("task=B"));
        assert!(status.contains("task=C"));
    }

    // ── JSON deserialization ────────────────────────────────────────────

    #[test]
    fn json_deserialization() {
        let json = r#"{
            "tasks": [
                {"id": "step1", "prompt": "Hello", "workload": "fast", "deps": []},
                {"id": "step2", "prompt": "World", "deps": ["step1"]}
            ],
            "failure_policy": "best_effort"
        }"#;
        let graph: TaskGraphDef = serde_json::from_str(json).expect("parse");
        assert_eq!(graph.tasks.len(), 2);
        assert_eq!(graph.failure_policy, FailurePolicy::BestEffort);
        assert_eq!(graph.tasks[0].workload.as_deref(), Some("fast"));
        assert!(graph.tasks[1].workload.is_none());
    }

    #[test]
    fn json_default_policy_is_fail_fast() {
        let json = r#"{"tasks": [{"id": "a", "prompt": "hi"}]}"#;
        let graph: TaskGraphDef = serde_json::from_str(json).expect("parse");
        assert_eq!(graph.failure_policy, FailurePolicy::FailFast);
    }

    // ── Workload parsing ────────────────────────────────────────────────

    #[test]
    fn workload_parsing() {
        assert!(matches!(parse_workload_str(Some("fast")), WorkloadClass::Fast));
        assert!(matches!(parse_workload_str(Some("CODE")), WorkloadClass::Code));
        assert!(matches!(parse_workload_str(Some("reasoning")), WorkloadClass::Reasoning));
        assert!(matches!(parse_workload_str(None), WorkloadClass::General));
        assert!(matches!(parse_workload_str(Some("unknown")), WorkloadClass::General));
    }

    // ── Context building ────────────────────────────────────────────────

    #[test]
    fn build_prompt_injects_dependency_output() {
        let task = TaskNodeDef {
            id: "D".into(),
            prompt: "Summarise everything".into(),
            workload: None,
            deps: vec!["A".into(), "B".into()],
        };
        let mut outputs = HashMap::new();
        outputs.insert("A".to_string(), "output A".to_string());
        outputs.insert("B".to_string(), "output B".to_string());

        let prompt = build_task_prompt(&task, &outputs);
        assert!(prompt.contains("output A"));
        assert!(prompt.contains("output B"));
        assert!(prompt.contains("Summarise everything"));
    }

    #[test]
    fn build_prompt_without_deps_returns_raw() {
        let task = TaskNodeDef {
            id: "root".into(),
            prompt: "do it".into(),
            workload: None,
            deps: vec![],
        };
        let prompt = build_task_prompt(&task, &HashMap::new());
        assert_eq!(prompt, "do it");
    }
}
