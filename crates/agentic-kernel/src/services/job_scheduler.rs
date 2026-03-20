use std::collections::{BTreeMap, BTreeSet, HashMap};

use agentic_control_models::{ScheduleJobResult, ScheduledJobRunView, ScheduledJobView};
use chrono::{DateTime, Datelike, Duration as ChronoDuration, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::orchestrator::{FailurePolicy, Orchestrator, TaskGraphDef};
use crate::storage::{
    current_timestamp_ms, NewScheduledJobRecord, StorageService, StoredScheduledJob,
    StoredScheduledJobRun,
};

const DEFAULT_JOB_TIMEOUT_MS: u64 = 15 * 60 * 1_000;
const DEFAULT_JOB_BACKOFF_MS: u64 = 30 * 1_000;
const MAX_RECENT_RUNS: usize = 8;
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
    jobs: BTreeMap<u64, ScheduledJob>,
    orchestration_to_job: HashMap<u64, u64>,
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
    minute: CronField,
    hour: CronField,
    day_of_month: CronField,
    month: CronField,
    day_of_week: CronField,
}

#[derive(Debug, Clone)]
struct CronField {
    values: BTreeSet<u32>,
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

    pub fn due_job_ids(&self, now_ms: i64) -> Vec<u64> {
        self.jobs
            .values()
            .filter(|job| {
                job.enabled
                    && matches!(
                        job.state,
                        ScheduledJobState::Idle | ScheduledJobState::RetryWait
                    )
                    && job.next_run_at_ms.is_some_and(|next| next <= now_ms)
            })
            .map(|job| job.job_id)
            .collect()
    }

    pub fn timeout_job_ids(&self, now_ms: i64) -> Vec<u64> {
        self.jobs
            .values()
            .filter(|job| {
                job.enabled
                    && job.state == ScheduledJobState::Running
                    && job
                        .active_deadline_at_ms
                        .is_some_and(|deadline| deadline <= now_ms)
            })
            .map(|job| job.job_id)
            .collect()
    }

    pub fn next_due_at_ms(&self) -> Option<i64> {
        self.jobs
            .values()
            .filter_map(|job| {
                if job.enabled
                    && matches!(
                        job.state,
                        ScheduledJobState::Idle | ScheduledJobState::RetryWait
                    )
                {
                    job.next_run_at_ms
                } else {
                    None
                }
            })
            .min()
    }

    pub fn next_timeout_at_ms(&self) -> Option<(u64, i64)> {
        self.jobs
            .values()
            .filter_map(|job| {
                if job.enabled && job.state == ScheduledJobState::Running {
                    job.active_deadline_at_ms
                        .map(|deadline| (job.job_id, deadline))
                } else {
                    None
                }
            })
            .min_by_key(|(_, deadline)| *deadline)
    }

    pub fn orchestration_ids(&self) -> Vec<u64> {
        self.orchestration_to_job.keys().copied().collect()
    }

    pub fn dispatch_plan(&self, job_id: u64) -> Result<DueJobDispatch, String> {
        let Some(job) = self.jobs.get(&job_id) else {
            return Err(format!("Scheduled job {} not found", job_id));
        };
        let trigger_at_ms = job
            .current_trigger_at_ms
            .or(job.next_run_at_ms)
            .ok_or_else(|| format!("Scheduled job {} has no trigger time", job_id))?;
        let attempt = job.current_attempt.saturating_add(1);
        let workflow = serde_json::from_str::<TaskGraphDef>(&job.workflow_payload)
            .map_err(|err| format!("Invalid persisted workflow payload: {}", err))?;
        Ok(DueJobDispatch {
            job_id,
            trigger_at_ms,
            attempt,
            workflow,
        })
    }

    pub fn mark_started(
        &mut self,
        storage: &mut StorageService,
        job_id: u64,
        trigger_at_ms: i64,
        attempt: u32,
        orchestration_id: u64,
    ) -> Result<(), String> {
        let now_ms = current_timestamp_ms();
        let Some(job) = self.jobs.get_mut(&job_id) else {
            return Err(format!("Scheduled job {} not found", job_id));
        };
        let deadline_at_ms = now_ms + job.timeout_ms as i64;
        let mut run = ScheduledJobRun {
            run_id: 0,
            trigger_at_ms,
            attempt,
            status: "running".to_string(),
            started_at_ms: Some(now_ms),
            completed_at_ms: None,
            orchestration_id: Some(orchestration_id),
            deadline_at_ms: Some(deadline_at_ms),
            error: None,
        };
        let run_id = storage
            .insert_scheduled_job_run(&run.to_stored(job_id))
            .map_err(|err| err.to_string())?;
        run.run_id = run_id;

        job.state = ScheduledJobState::Running;
        job.current_trigger_at_ms = Some(trigger_at_ms);
        job.current_attempt = attempt;
        job.active_run_id = Some(run_id);
        job.active_orchestration_id = Some(orchestration_id);
        job.active_deadline_at_ms = Some(deadline_at_ms);
        job.last_run_started_at_ms = Some(now_ms);
        job.last_run_status = Some("running".to_string());
        job.last_error = None;
        job.next_run_at_ms = None;
        job.updated_at_ms = now_ms;
        job.push_recent_run(run);

        storage
            .save_scheduled_job(&job.to_stored())
            .map_err(|err| err.to_string())?;
        self.orchestration_to_job.insert(orchestration_id, job_id);
        Ok(())
    }

    pub fn mark_dispatch_failed(
        &mut self,
        storage: &mut StorageService,
        job_id: u64,
        trigger_at_ms: i64,
        attempt: u32,
        error: &str,
    ) -> Result<(), String> {
        let now_ms = current_timestamp_ms();
        let Some(job) = self.jobs.get_mut(&job_id) else {
            return Err(format!("Scheduled job {} not found", job_id));
        };
        let mut run = ScheduledJobRun {
            run_id: 0,
            trigger_at_ms,
            attempt,
            status: "failed".to_string(),
            started_at_ms: Some(now_ms),
            completed_at_ms: Some(now_ms),
            orchestration_id: None,
            deadline_at_ms: None,
            error: Some(error.to_string()),
        };
        let run_id = storage
            .insert_scheduled_job_run(&run.to_stored(job_id))
            .map_err(|err| err.to_string())?;
        run.run_id = run_id;
        job.push_recent_run(run);
        job.current_trigger_at_ms = Some(trigger_at_ms);
        job.current_attempt = attempt;
        job.transition_after_failure("failed", error, now_ms);
        storage
            .save_scheduled_job(&job.to_stored())
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn complete_orchestration(
        &mut self,
        storage: &mut StorageService,
        orchestration_id: u64,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), String> {
        let Some(job_id) = self.orchestration_to_job.remove(&orchestration_id) else {
            return Ok(());
        };
        let now_ms = current_timestamp_ms();
        let Some(job) = self.jobs.get_mut(&job_id) else {
            return Ok(());
        };
        let run_id = job
            .active_run_id
            .ok_or_else(|| format!("Scheduled job {} has no active run", job_id))?;
        let Some(run) = job.recent_runs.iter_mut().find(|run| run.run_id == run_id) else {
            return Err(format!(
                "Scheduled job {} missing recent run {}",
                job_id, run_id
            ));
        };
        run.status = status.to_string();
        run.completed_at_ms = Some(now_ms);
        run.error = error.map(ToOwned::to_owned);
        storage
            .save_scheduled_job_run(&run.to_stored(job_id))
            .map_err(|err| err.to_string())?;

        if status == "completed" {
            job.transition_after_success(now_ms);
        } else {
            job.transition_after_failure(status, error.unwrap_or(status), now_ms);
        }

        storage
            .save_scheduled_job(&job.to_stored())
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn set_enabled(
        &mut self,
        storage: &mut StorageService,
        job_id: u64,
        enabled: bool,
    ) -> Result<ScheduledJobView, String> {
        let now_ms = current_timestamp_ms();
        let Some(job) = self.jobs.get_mut(&job_id) else {
            return Err(format!("Scheduled job {} not found", job_id));
        };
        if job.state == ScheduledJobState::Running {
            return Err(format!(
                "Scheduled job {} is running and cannot change enabled state",
                job_id
            ));
        }

        job.enabled = enabled;
        if enabled {
            job.state = ScheduledJobState::Idle;
            job.next_run_at_ms = job.trigger.next_after(now_ms);
        } else {
            job.state = ScheduledJobState::Disabled;
            job.next_run_at_ms = None;
            job.current_trigger_at_ms = None;
            job.current_attempt = 0;
        }
        job.updated_at_ms = now_ms;
        storage
            .save_scheduled_job(&job.to_stored())
            .map_err(|err| err.to_string())?;
        Ok(job.to_view())
    }

    pub fn delete_job(&mut self, storage: &mut StorageService, job_id: u64) -> Result<(), String> {
        let Some(job) = self.jobs.get(&job_id) else {
            return Err(format!("Scheduled job {} not found", job_id));
        };
        if job.state == ScheduledJobState::Running || job.active_orchestration_id.is_some() {
            return Err(format!(
                "Scheduled job {} is still running; stop the active orchestration first",
                job_id
            ));
        }

        self.jobs.remove(&job_id);
        storage
            .delete_scheduled_job(job_id)
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub fn mark_timed_out(
        &mut self,
        storage: &mut StorageService,
        job_id: u64,
    ) -> Result<Option<u64>, String> {
        let now_ms = current_timestamp_ms();
        let Some(job) = self.jobs.get_mut(&job_id) else {
            return Ok(None);
        };
        let orchestration_id = job.active_orchestration_id;
        if let Some(active_orchestration_id) = orchestration_id {
            self.orchestration_to_job.remove(&active_orchestration_id);
        }
        let run_id = job
            .active_run_id
            .ok_or_else(|| format!("Scheduled job {} has no active run", job_id))?;
        let Some(run) = job.recent_runs.iter_mut().find(|run| run.run_id == run_id) else {
            return Err(format!(
                "Scheduled job {} missing recent run {}",
                job_id, run_id
            ));
        };
        run.status = "timed_out".to_string();
        run.completed_at_ms = Some(now_ms);
        run.error = Some("scheduler_timeout".to_string());
        storage
            .save_scheduled_job_run(&run.to_stored(job_id))
            .map_err(|err| err.to_string())?;
        job.transition_after_failure("timed_out", "scheduler_timeout", now_ms);
        storage
            .save_scheduled_job(&job.to_stored())
            .map_err(|err| err.to_string())?;
        Ok(orchestration_id)
    }

    pub fn scheduled_jobs(&self) -> Vec<&ScheduledJob> {
        self.jobs.values().collect()
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

    fn to_stored(&self) -> StoredScheduledJob {
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

    fn push_recent_run(&mut self, run: ScheduledJobRun) {
        self.recent_runs
            .retain(|existing| existing.run_id != run.run_id);
        self.recent_runs.insert(0, run);
        self.recent_runs
            .sort_by(|left, right| right.run_id.cmp(&left.run_id));
        self.recent_runs.truncate(MAX_RECENT_RUNS);
    }

    fn transition_after_success(&mut self, now_ms: i64) {
        let is_one_shot = matches!(self.trigger, ScheduledJobTrigger::At { .. });
        self.state = if self.enabled {
            ScheduledJobState::Idle
        } else {
            ScheduledJobState::Disabled
        };
        self.next_run_at_ms = if self.enabled && !is_one_shot {
            self.trigger.next_after(now_ms)
        } else {
            None
        };
        self.current_trigger_at_ms = None;
        self.current_attempt = 0;
        self.active_run_id = None;
        self.active_orchestration_id = None;
        self.active_deadline_at_ms = None;
        self.last_run_completed_at_ms = Some(now_ms);
        self.last_run_status = Some("completed".to_string());
        self.last_error = None;
        self.consecutive_failures = 0;
        self.updated_at_ms = now_ms;
        if is_one_shot {
            self.enabled = false;
            self.state = ScheduledJobState::Completed;
        }
    }

    fn transition_after_failure(&mut self, status: &str, error: &str, now_ms: i64) {
        let is_one_shot = matches!(self.trigger, ScheduledJobTrigger::At { .. });
        self.active_run_id = None;
        self.active_orchestration_id = None;
        self.active_deadline_at_ms = None;
        self.last_run_completed_at_ms = Some(now_ms);
        self.last_run_status = Some(status.to_string());
        self.last_error = Some(error.to_string());
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.updated_at_ms = now_ms;

        if self.enabled && self.current_attempt <= self.max_retries {
            self.state = ScheduledJobState::RetryWait;
            self.next_run_at_ms = Some(now_ms + self.backoff_ms as i64);
            return;
        }

        self.current_attempt = 0;
        self.current_trigger_at_ms = None;
        self.next_run_at_ms = if self.enabled && !is_one_shot {
            self.trigger.next_after(now_ms)
        } else {
            None
        };
        self.state = if self.enabled && self.next_run_at_ms.is_some() {
            ScheduledJobState::Idle
        } else if is_one_shot {
            self.enabled = false;
            ScheduledJobState::Completed
        } else {
            ScheduledJobState::Disabled
        };
    }
}

impl ScheduledJobRun {
    fn to_stored(&self, job_id: u64) -> StoredScheduledJobRun {
        StoredScheduledJobRun {
            run_id: self.run_id,
            job_id,
            trigger_at_ms: self.trigger_at_ms,
            attempt: self.attempt,
            status: self.status.clone(),
            started_at_ms: self.started_at_ms,
            completed_at_ms: self.completed_at_ms,
            orchestration_id: self.orchestration_id,
            deadline_at_ms: self.deadline_at_ms,
            error: self.error.clone(),
        }
    }
}

impl From<StoredScheduledJobRun> for ScheduledJobRun {
    fn from(value: StoredScheduledJobRun) -> Self {
        Self {
            run_id: value.run_id,
            trigger_at_ms: value.trigger_at_ms,
            attempt: value.attempt,
            status: value.status,
            started_at_ms: value.started_at_ms,
            completed_at_ms: value.completed_at_ms,
            orchestration_id: value.orchestration_id,
            deadline_at_ms: value.deadline_at_ms,
            error: value.error,
        }
    }
}

impl From<ScheduledJobRun> for ScheduledJobRunView {
    fn from(value: ScheduledJobRun) -> Self {
        Self {
            run_id: value.run_id,
            trigger_at_ms: value.trigger_at_ms,
            attempt: value.attempt,
            status: value.status,
            started_at_ms: value.started_at_ms,
            completed_at_ms: value.completed_at_ms,
            orchestration_id: value.orchestration_id,
            deadline_at_ms: value.deadline_at_ms,
            error: value.error,
        }
    }
}

impl ScheduledJobTrigger {
    fn from_request(request: ScheduledJobTriggerInput, now_ms: i64) -> Result<Self, String> {
        match request {
            ScheduledJobTriggerInput::At { at_ms } => {
                if at_ms <= now_ms {
                    return Err("at_ms must be in the future".to_string());
                }
                Ok(Self::At { at_ms })
            }
            ScheduledJobTriggerInput::Interval {
                every_ms,
                starts_at_ms,
            } => {
                if every_ms == 0 {
                    return Err("every_ms must be > 0".to_string());
                }
                Ok(Self::Interval {
                    every_ms,
                    anchor_ms: starts_at_ms.unwrap_or(now_ms),
                })
            }
            ScheduledJobTriggerInput::Cron { expression } => {
                let schedule = CronSchedule::parse(&expression)?;
                Ok(Self::Cron {
                    expression,
                    schedule,
                })
            }
        }
    }

    fn from_stored(kind: &str, payload: &str) -> Result<Self, String> {
        match kind {
            "at" => {
                #[derive(Deserialize)]
                struct AtPayload {
                    at_ms: i64,
                }
                let parsed =
                    serde_json::from_str::<AtPayload>(payload).map_err(|err| err.to_string())?;
                Ok(Self::At {
                    at_ms: parsed.at_ms,
                })
            }
            "interval" => {
                #[derive(Deserialize)]
                struct IntervalPayload {
                    every_ms: u64,
                    anchor_ms: i64,
                }
                let parsed = serde_json::from_str::<IntervalPayload>(payload)
                    .map_err(|err| err.to_string())?;
                Ok(Self::Interval {
                    every_ms: parsed.every_ms.max(1),
                    anchor_ms: parsed.anchor_ms,
                })
            }
            "cron" => {
                #[derive(Deserialize)]
                struct CronPayload {
                    expression: String,
                }
                let parsed =
                    serde_json::from_str::<CronPayload>(payload).map_err(|err| err.to_string())?;
                let schedule = CronSchedule::parse(&parsed.expression)?;
                Ok(Self::Cron {
                    expression: parsed.expression,
                    schedule,
                })
            }
            other => Err(format!("Unsupported trigger kind '{}'", other)),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::At { .. } => "at",
            Self::Interval { .. } => "interval",
            Self::Cron { .. } => "cron",
        }
    }

    fn label(&self) -> String {
        match self {
            Self::At { at_ms } => format!("at {}", at_ms),
            Self::Interval { every_ms, .. } => format!("every {}s", every_ms / 1_000),
            Self::Cron { expression, .. } => format!("cron {}", expression),
        }
    }

    fn to_payload_json(&self) -> Result<String, String> {
        match self {
            Self::At { at_ms } => serde_json::to_string(&serde_json::json!({ "at_ms": at_ms }))
                .map_err(|err| err.to_string()),
            Self::Interval {
                every_ms,
                anchor_ms,
            } => serde_json::to_string(&serde_json::json!({
                "every_ms": every_ms,
                "anchor_ms": anchor_ms,
            }))
            .map_err(|err| err.to_string()),
            Self::Cron { expression, .. } => serde_json::to_string(&serde_json::json!({
                "expression": expression,
            }))
            .map_err(|err| err.to_string()),
        }
    }

    fn next_after(&self, after_ms: i64) -> Option<i64> {
        match self {
            Self::At { at_ms } => (*at_ms > after_ms).then_some(*at_ms),
            Self::Interval {
                every_ms,
                anchor_ms,
            } => {
                if after_ms < *anchor_ms {
                    return Some(*anchor_ms);
                }
                let every_ms = *every_ms as i64;
                let delta = after_ms.saturating_sub(*anchor_ms);
                let ticks = delta.div_euclid(every_ms) + 1;
                Some(anchor_ms.saturating_add(ticks.saturating_mul(every_ms)))
            }
            Self::Cron { schedule, .. } => schedule.next_after(after_ms),
        }
    }
}

impl ScheduledJobState {
    fn from_str(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "retry_wait" => Self::RetryWait,
            "completed" => Self::Completed,
            "disabled" => Self::Disabled,
            _ => Self::Idle,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::RetryWait => "retry_wait",
            Self::Completed => "completed",
            Self::Disabled => "disabled",
        }
    }
}

impl CronSchedule {
    fn parse(expression: &str) -> Result<Self, String> {
        let parts = expression.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 5 {
            return Err("Cron expression must have 5 fields: min hour dom month dow".to_string());
        }
        Ok(Self {
            minute: CronField::parse(parts[0], 0, 59)?,
            hour: CronField::parse(parts[1], 0, 23)?,
            day_of_month: CronField::parse(parts[2], 1, 31)?,
            month: CronField::parse(parts[3], 1, 12)?,
            day_of_week: CronField::parse(parts[4], 0, 6)?,
        })
    }

    fn next_after(&self, after_ms: i64) -> Option<i64> {
        let next_minute_ms = after_ms.div_euclid(60_000).saturating_add(1) * 60_000;
        let mut candidate = DateTime::<Utc>::from_timestamp_millis(next_minute_ms)?;
        for _ in 0..(366 * 24 * 60) {
            if self.matches(candidate) {
                return Some(candidate.timestamp_millis());
            }
            candidate += ChronoDuration::minutes(1);
        }
        None
    }

    fn matches(&self, candidate: DateTime<Utc>) -> bool {
        self.minute.matches(candidate.minute())
            && self.hour.matches(candidate.hour())
            && self.day_of_month.matches(candidate.day())
            && self.month.matches(candidate.month())
            && self
                .day_of_week
                .matches(candidate.weekday().num_days_from_sunday())
    }
}

impl CronField {
    fn parse(input: &str, min: u32, max: u32) -> Result<Self, String> {
        let mut values = BTreeSet::new();
        for part in input.split(',') {
            let part = part.trim();
            if part.is_empty() {
                return Err("Cron field cannot be empty".to_string());
            }
            if let Some((base, step_raw)) = part.split_once('/') {
                let step = step_raw
                    .parse::<u32>()
                    .map_err(|_| format!("Invalid cron step '{}'", step_raw))?;
                if step == 0 {
                    return Err("Cron step must be > 0".to_string());
                }
                let (range_start, range_end) = parse_range(base, min, max)?;
                let mut current = range_start;
                while current <= range_end {
                    values.insert(current);
                    current = current.saturating_add(step);
                    if current == 0 {
                        break;
                    }
                }
                continue;
            }

            let (range_start, range_end) = parse_range(part, min, max)?;
            for value in range_start..=range_end {
                values.insert(value);
            }
        }

        if values.is_empty() {
            return Err("Cron field resolved to an empty set".to_string());
        }

        Ok(Self { values })
    }

    fn matches(&self, value: u32) -> bool {
        self.values.contains(&value)
    }
}

fn parse_range(input: &str, min: u32, max: u32) -> Result<(u32, u32), String> {
    if input == "*" {
        return Ok((min, max));
    }
    if let Some((start_raw, end_raw)) = input.split_once('-') {
        let start = parse_cron_number(start_raw, min, max)?;
        let end = parse_cron_number(end_raw, min, max)?;
        if start > end {
            return Err(format!("Invalid cron range '{}'", input));
        }
        return Ok((start, end));
    }
    let value = parse_cron_number(input, min, max)?;
    Ok((value, value))
}

fn parse_cron_number(raw: &str, min: u32, max: u32) -> Result<u32, String> {
    let value = raw
        .parse::<u32>()
        .map_err(|_| format!("Invalid cron value '{}'", raw))?;
    if value < min || value > max {
        return Err(format!(
            "Cron value '{}' out of range {}..={}",
            raw, min, max
        ));
    }
    Ok(value)
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
