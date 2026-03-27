use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct LobbyOrchestrationSummary {
    pub orchestration_id: u64,
    pub total: usize,
    pub completed: usize,
    pub running: usize,
    pub pending: usize,
    pub failed: usize,
    pub skipped: usize,
    pub finished: bool,
    pub elapsed_label: String,
    pub policy: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceOrchestrationSnapshot {
    pub orchestration_id: u64,
    pub task_id: String,
    pub total: usize,
    pub completed: usize,
    pub running: usize,
    pub pending: usize,
    pub failed: usize,
    pub skipped: usize,
    pub finished: bool,
    pub elapsed_secs: f64,
    pub policy: String,
    pub tasks: Vec<WorkspaceOrchestrationTask>,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceOrchestrationTask {
    pub task: String,
    pub status: String,
    pub pid: Option<u64>,
}
