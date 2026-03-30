use std::collections::{BTreeMap, BTreeSet, HashMap};

use agentic_control_models::{ScheduleJobResult, ScheduledJobView};
use serde::{Deserialize, Serialize};

use crate::orchestrator::{FailurePolicy, Orchestrator, TaskGraphDef};
use crate::storage::{
    current_timestamp_ms, NewScheduledJobRecord, StorageService, StoredScheduledJob,
};

const DEFAULT_JOB_TIMEOUT_MS: u64 = 15 * 60 * 1_000;
const DEFAULT_JOB_BACKOFF_MS: u64 = 30 * 1_000;
pub(super) const MAX_RECENT_RUNS: usize = 8;
pub(crate) const SCHEDULER_SYSTEM_OWNER_ID: usize = 0;

#[derive(Debug, Clone)]
pub(crate) struct ScheduledWorkflowJobRequest {
    pub name: String,
    pub workflow: TaskGraphDef,
    pub workflow_payload: String,
    pub trigger: ScheduledJobTriggerInput,
    pub timeout_ms: Option<u64>,
    pub max_retries: Option<u32>,
    pub backoff_ms: Option<u64>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduledJobTriggerInput {
    At {
        at_ms: i64,
    },
    Interval {
        every_ms: u64,
        #[serde(default)]
        starts_at_ms: Option<i64>,
    },
    Cron {
        expression: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct JobScheduler {
    pub(super) jobs: BTreeMap<u64, ScheduledJob>,
    pub(super) orchestration_to_job: HashMap<u64, u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct ScheduledJob {
    pub job_id: u64,
    pub name: String,
    pub target_kind: String,
    pub workflow_payload: String,
    pub trigger: ScheduledJobTrigger,
    pub timeout_ms: u64,
    pub max_retries: u32,
    pub backoff_ms: u64,
    pub enabled: bool,
    pub state: ScheduledJobState,
    pub next_run_at_ms: Option<i64>,
    pub current_trigger_at_ms: Option<i64>,
    pub current_attempt: u32,
    pub active_run_id: Option<u64>,
    pub active_orchestration_id: Option<u64>,
    pub active_deadline_at_ms: Option<i64>,
    pub last_run_started_at_ms: Option<i64>,
    pub last_run_completed_at_ms: Option<i64>,
    pub last_run_status: Option<String>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub recent_runs: Vec<ScheduledJobRun>,
}

#[derive(Debug, Clone)]
pub(crate) struct ScheduledJobRun {
    pub run_id: u64,
    pub trigger_at_ms: i64,
    pub attempt: u32,
    pub status: String,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: Option<i64>,
    pub orchestration_id: Option<u64>,
    pub deadline_at_ms: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DueJobDispatch {
    pub job_id: u64,
    pub trigger_at_ms: i64,
    pub attempt: u32,
    pub workflow: TaskGraphDef,
}

#[derive(Debug, Clone)]
pub(crate) enum ScheduledJobTrigger {
    At {
        at_ms: i64,
    },
    Interval {
        every_ms: u64,
        anchor_ms: i64,
    },
    Cron {
        expression: String,
        schedule: CronSchedule,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct CronSchedule {
    pub(super) minute: CronField,
    pub(super) hour: CronField,
    pub(super) day_of_month: CronField,
    pub(super) month: CronField,
    pub(super) day_of_week: CronField,
}

#[derive(Debug, Clone)]
pub(super) struct CronField {
    pub(super) values: BTreeSet<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScheduledJobState {
    Idle,
    Running,
    RetryWait,
    Completed,
    Disabled,
}

impl JobScheduler {
    pub fn new() -> Self {
        Self {
            jobs: BTreeMap::new(),
            orchestration_to_job: HashMap::new(),
        }
    }

    pub fn load(storage: &mut StorageService) -> Result<Self, String> {
        let now_ms = current_timestamp_ms();
        let mut scheduler = Self::new();
        let stored_jobs = storage
            .load_scheduled_jobs()
            .map_err(|err| err.to_string())?;

        for stored_job in stored_jobs {
            let recent_runs = storage
                .load_scheduled_job_runs(stored_job.job_id, MAX_RECENT_RUNS)
                .map_err(|err| err.to_string())?
                .into_iter()
                .map(ScheduledJobRun::from)
                .collect::<Vec<_>>();

            let (job, needs_sync) = ScheduledJob::from_stored(stored_job, recent_runs, now_ms)?;
            if needs_sync {
                storage
                    .save_scheduled_job(&job.to_stored())
                    .map_err(|err| err.to_string())?;
            }
            if let Some(orch_id) = job.active_orchestration_id {
                scheduler.orchestration_to_job.insert(orch_id, job.job_id);
            }
            scheduler.jobs.insert(job.job_id, job);
        }

        Ok(scheduler)
    }

    pub fn schedule_workflow_job(
        &mut self,
        storage: &mut StorageService,
        request: ScheduledWorkflowJobRequest,
    ) -> Result<ScheduleJobResult, String> {
        validate_workflow_definition(&request.workflow)?;

        let now_ms = current_timestamp_ms();
        let trigger = ScheduledJobTrigger::from_request(request.trigger, now_ms)
            .map_err(|err| err.to_string())?;
        let timeout_ms = request.timeout_ms.unwrap_or(DEFAULT_JOB_TIMEOUT_MS).max(1);
        let backoff_ms = request.backoff_ms.unwrap_or(DEFAULT_JOB_BACKOFF_MS).max(1);
        let next_run_at_ms = if request.enabled {
            trigger
                .next_after(now_ms - 1)
                .ok_or_else(|| "The requested trigger does not produce a future run.".to_string())?
        } else {
            0
        };
        let state = if request.enabled {
            ScheduledJobState::Idle
        } else {
            ScheduledJobState::Disabled
        };
        let stored = storage
            .insert_scheduled_job(&NewScheduledJobRecord {
                name: request.name.trim().to_string(),
                target_kind: "workflow".to_string(),
                workflow_payload: request.workflow_payload,
                trigger_kind: trigger.kind().to_string(),
                trigger_payload: trigger.to_payload_json()?,
                timeout_ms,
                max_retries: request.max_retries.unwrap_or(0),
                backoff_ms,
                enabled: request.enabled,
                state: state.as_str().to_string(),
                next_run_at_ms: if request.enabled {
                    Some(next_run_at_ms)
                } else {
                    None
                },
                current_trigger_at_ms: None,
                current_attempt: 0,
                active_run_id: None,
                active_orchestration_id: None,
                active_deadline_at_ms: None,
                last_run_started_at_ms: None,
                last_run_completed_at_ms: None,
                last_run_status: None,
                last_error: None,
                consecutive_failures: 0,
                created_at_ms: now_ms,
                updated_at_ms: now_ms,
            })
            .map_err(|err| err.to_string())?;
        let job = ScheduledJob::from_stored(stored, Vec::new(), now_ms)?.0;
        let job_id = job.job_id;
        self.jobs.insert(job_id, job);
        Ok(ScheduleJobResult {
            job_id,
            next_run_at_ms: self.jobs.get(&job_id).and_then(|job| job.next_run_at_ms),
            trigger_kind: trigger.kind().to_string(),
        })
    }
}

impl ScheduledJob {
    fn from_stored(
        stored: StoredScheduledJob,
        recent_runs: Vec<ScheduledJobRun>,
        now_ms: i64,
    ) -> Result<(Self, bool), String> {
        let trigger =
            ScheduledJobTrigger::from_stored(&stored.trigger_kind, &stored.trigger_payload)?;
        let mut job = Self {
            job_id: stored.job_id,
            name: stored.name,
            target_kind: stored.target_kind,
            workflow_payload: stored.workflow_payload,
            trigger,
            timeout_ms: stored.timeout_ms.max(1),
            max_retries: stored.max_retries,
            backoff_ms: stored.backoff_ms.max(1),
            enabled: stored.enabled,
            state: ScheduledJobState::from_str(&stored.state),
            next_run_at_ms: stored.next_run_at_ms,
            current_trigger_at_ms: stored.current_trigger_at_ms,
            current_attempt: stored.current_attempt,
            active_run_id: stored.active_run_id,
            active_orchestration_id: stored.active_orchestration_id,
            active_deadline_at_ms: stored.active_deadline_at_ms,
            last_run_started_at_ms: stored.last_run_started_at_ms,
            last_run_completed_at_ms: stored.last_run_completed_at_ms,
            last_run_status: stored.last_run_status,
            last_error: stored.last_error,
            consecutive_failures: stored.consecutive_failures,
            created_at_ms: stored.created_at_ms,
            updated_at_ms: stored.updated_at_ms,
            recent_runs,
        };
        let mut needs_sync = false;

        if !job.enabled {
            if job.state != ScheduledJobState::Disabled || job.next_run_at_ms.is_some() {
                job.state = ScheduledJobState::Disabled;
                job.next_run_at_ms = None;
                job.updated_at_ms = now_ms;
                needs_sync = true;
            }
        } else if job.state == ScheduledJobState::Running {
            job.transition_after_failure("interrupted", "kernel_restarted", now_ms);
            needs_sync = true;
        } else if matches!(
            job.state,
            ScheduledJobState::Idle | ScheduledJobState::RetryWait
        ) && job.next_run_at_ms.is_none()
        {
            job.next_run_at_ms = job.trigger.next_after(now_ms);
            job.updated_at_ms = now_ms;
            needs_sync = true;
        }

        Ok((job, needs_sync))
    }

    pub fn to_view(&self) -> ScheduledJobView {
        ScheduledJobView {
            job_id: self.job_id,
            name: self.name.clone(),
            target_kind: self.target_kind.clone(),
            trigger_kind: self.trigger.kind().to_string(),
            trigger_label: self.trigger.label(),
            enabled: self.enabled,
            state: self.state.as_str().to_string(),
            next_run_at_ms: self.next_run_at_ms,
            current_trigger_at_ms: self.current_trigger_at_ms,
            current_attempt: self.current_attempt,
            timeout_ms: self.timeout_ms,
            max_retries: self.max_retries,
            backoff_ms: self.backoff_ms,
            last_run_started_at_ms: self.last_run_started_at_ms,
            last_run_completed_at_ms: self.last_run_completed_at_ms,
            last_run_status: self.last_run_status.clone(),
            last_error: self.last_error.clone(),
            consecutive_failures: self.consecutive_failures,
            active_orchestration_id: self.active_orchestration_id,
            recent_runs: self.recent_runs.iter().cloned().map(Into::into).collect(),
        }
    }

    pub(super) fn to_stored(&self) -> StoredScheduledJob {
        StoredScheduledJob {
            job_id: self.job_id,
            name: self.name.clone(),
            target_kind: self.target_kind.clone(),
            workflow_payload: self.workflow_payload.clone(),
            trigger_kind: self.trigger.kind().to_string(),
            trigger_payload: self
                .trigger
                .to_payload_json()
                .unwrap_or_else(|_| "{}".to_string()),
            timeout_ms: self.timeout_ms,
            max_retries: self.max_retries,
            backoff_ms: self.backoff_ms,
            enabled: self.enabled,
            state: self.state.as_str().to_string(),
            next_run_at_ms: self.next_run_at_ms,
            current_trigger_at_ms: self.current_trigger_at_ms,
            current_attempt: self.current_attempt,
            active_run_id: self.active_run_id,
            active_orchestration_id: self.active_orchestration_id,
            active_deadline_at_ms: self.active_deadline_at_ms,
            last_run_started_at_ms: self.last_run_started_at_ms,
            last_run_completed_at_ms: self.last_run_completed_at_ms,
            last_run_status: self.last_run_status.clone(),
            last_error: self.last_error.clone(),
            consecutive_failures: self.consecutive_failures,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

fn validate_workflow_definition(workflow: &TaskGraphDef) -> Result<(), String> {
    let mut validator = Orchestrator::new();
    validator
        .register(
            TaskGraphDef {
                tasks: workflow.tasks.clone(),
                failure_policy: match workflow.failure_policy {
                    FailurePolicy::FailFast => FailurePolicy::FailFast,
                    FailurePolicy::BestEffort => FailurePolicy::BestEffort,
                },
            },
            SCHEDULER_SYSTEM_OWNER_ID,
        )
        .map(|_| ())
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageService;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn interval_jobs_persist_and_reload_with_next_run() {
        let dir = make_temp_dir("agenticos_job_scheduler_reload");
        let db_path = dir.join("agenticos.db");
        let workflow_json = sample_workflow_json();

        let mut storage = StorageService::open(&db_path).expect("open storage");
        let mut scheduler = JobScheduler::new();
        let result = scheduler
            .schedule_workflow_job(
                &mut storage,
                ScheduledWorkflowJobRequest {
                    name: "heartbeat".to_string(),
                    workflow: sample_workflow(),
                    workflow_payload: serde_json::to_string(&workflow_json)
                        .expect("serialize workflow"),
                    trigger: ScheduledJobTriggerInput::Interval {
                        every_ms: 1_000,
                        starts_at_ms: None,
                    },
                    timeout_ms: Some(2_000),
                    max_retries: Some(1),
                    backoff_ms: Some(500),
                    enabled: true,
                },
            )
            .expect("schedule interval job");

        assert_eq!(result.trigger_kind, "interval");
        assert!(result.next_run_at_ms.is_some());
        assert_eq!(scheduler.scheduled_jobs().len(), 1);

        drop(scheduler);
        drop(storage);

        let mut reopened = StorageService::open(&db_path).expect("reopen storage");
        let reloaded = JobScheduler::load(&mut reopened).expect("reload scheduler");
        let job = reloaded
            .scheduled_jobs()
            .into_iter()
            .next()
            .expect("reloaded job");
        assert_eq!(job.name, "heartbeat");
        assert_eq!(job.state, ScheduledJobState::Idle);
        assert!(job.next_run_at_ms.is_some());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dispatch_failure_enters_retry_wait_and_records_error() {
        let dir = make_temp_dir("agenticos_job_scheduler_retry");
        let db_path = dir.join("agenticos.db");

        let mut storage = StorageService::open(&db_path).expect("open storage");
        let mut scheduler = JobScheduler::new();
        let result = scheduler
            .schedule_workflow_job(
                &mut storage,
                ScheduledWorkflowJobRequest {
                    name: "retryable".to_string(),
                    workflow: sample_workflow(),
                    workflow_payload: serde_json::to_string(&sample_workflow_json())
                        .expect("serialize workflow"),
                    trigger: ScheduledJobTriggerInput::Interval {
                        every_ms: 10_000,
                        starts_at_ms: None,
                    },
                    timeout_ms: Some(5_000),
                    max_retries: Some(2),
                    backoff_ms: Some(250),
                    enabled: true,
                },
            )
            .expect("schedule retryable job");
        let plan = scheduler
            .dispatch_plan(result.job_id)
            .expect("dispatch plan");

        scheduler
            .mark_dispatch_failed(
                &mut storage,
                plan.job_id,
                plan.trigger_at_ms,
                plan.attempt,
                "no_model_loaded",
            )
            .expect("mark dispatch failed");

        let job = scheduler
            .scheduled_jobs()
            .into_iter()
            .find(|job| job.job_id == result.job_id)
            .expect("job exists");
        assert_eq!(job.state, ScheduledJobState::RetryWait);
        assert_eq!(job.current_attempt, 1);
        assert_eq!(job.last_error.as_deref(), Some("no_model_loaded"));
        assert_eq!(
            job.recent_runs.first().map(|run| run.status.as_str()),
            Some("failed")
        );
        assert!(job.next_run_at_ms.is_some());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn timeout_marks_running_job_and_returns_orchestration_id() {
        let dir = make_temp_dir("agenticos_job_scheduler_timeout");
        let db_path = dir.join("agenticos.db");

        let mut storage = StorageService::open(&db_path).expect("open storage");
        let mut scheduler = JobScheduler::new();
        let result = scheduler
            .schedule_workflow_job(
                &mut storage,
                ScheduledWorkflowJobRequest {
                    name: "timed".to_string(),
                    workflow: sample_workflow(),
                    workflow_payload: serde_json::to_string(&sample_workflow_json())
                        .expect("serialize workflow"),
                    trigger: ScheduledJobTriggerInput::Interval {
                        every_ms: 5_000,
                        starts_at_ms: None,
                    },
                    timeout_ms: Some(1_000),
                    max_retries: Some(1),
                    backoff_ms: Some(300),
                    enabled: true,
                },
            )
            .expect("schedule timed job");
        let plan = scheduler
            .dispatch_plan(result.job_id)
            .expect("dispatch plan");

        scheduler
            .mark_started(
                &mut storage,
                plan.job_id,
                plan.trigger_at_ms,
                plan.attempt,
                77,
            )
            .expect("mark started");

        let timed_out_orch = scheduler
            .mark_timed_out(&mut storage, result.job_id)
            .expect("mark timed out");

        let job = scheduler
            .scheduled_jobs()
            .into_iter()
            .find(|job| job.job_id == result.job_id)
            .expect("job exists");
        assert_eq!(timed_out_orch, Some(77));
        assert_eq!(job.state, ScheduledJobState::RetryWait);
        assert_eq!(job.last_run_status.as_deref(), Some("timed_out"));
        assert_eq!(job.last_error.as_deref(), Some("scheduler_timeout"));
        assert!(job.next_run_at_ms.is_some());
        assert_eq!(
            job.recent_runs.first().map(|run| run.status.as_str()),
            Some("timed_out")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn one_shot_job_completes_and_disables_after_success() {
        let dir = make_temp_dir("agenticos_job_scheduler_one_shot");
        let db_path = dir.join("agenticos.db");

        let mut storage = StorageService::open(&db_path).expect("open storage");
        let mut scheduler = JobScheduler::new();
        let now_ms = current_timestamp_ms();
        let result = scheduler
            .schedule_workflow_job(
                &mut storage,
                ScheduledWorkflowJobRequest {
                    name: "one-shot".to_string(),
                    workflow: sample_workflow(),
                    workflow_payload: serde_json::to_string(&sample_workflow_json())
                        .expect("serialize workflow"),
                    trigger: ScheduledJobTriggerInput::At {
                        at_ms: now_ms + 60_000,
                    },
                    timeout_ms: Some(2_000),
                    max_retries: Some(0),
                    backoff_ms: Some(250),
                    enabled: true,
                },
            )
            .expect("schedule one-shot job");
        let plan = scheduler
            .dispatch_plan(result.job_id)
            .expect("dispatch plan");

        scheduler
            .mark_started(
                &mut storage,
                plan.job_id,
                plan.trigger_at_ms,
                plan.attempt,
                55,
            )
            .expect("mark started");
        scheduler
            .complete_orchestration(&mut storage, 55, "completed", None)
            .expect("complete job");

        let job = scheduler
            .scheduled_jobs()
            .into_iter()
            .find(|job| job.job_id == result.job_id)
            .expect("job exists");
        assert_eq!(job.state, ScheduledJobState::Completed);
        assert!(!job.enabled);
        assert!(job.next_run_at_ms.is_none());
        assert_eq!(job.last_run_status.as_deref(), Some("completed"));
        assert_eq!(
            job.recent_runs.first().map(|run| run.status.as_str()),
            Some("completed")
        );

        let _ = fs::remove_dir_all(dir);
    }

    fn sample_workflow() -> TaskGraphDef {
        serde_json::from_value(sample_workflow_json()).expect("workflow json")
    }

    fn sample_workflow_json() -> serde_json::Value {
        serde_json::json!({
            "failure_policy": "fail_fast",
            "tasks": [
                {
                    "id": "deliver",
                    "prompt": "Produce a deterministic report.",
                    "deps": []
                }
            ]
        })
    }

    fn make_temp_dir(prefix: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), timestamp));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
