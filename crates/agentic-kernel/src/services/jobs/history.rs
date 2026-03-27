use agentic_control_models::ScheduledJobRunView;

use crate::storage::{current_timestamp_ms, StorageService, StoredScheduledJobRun};

use super::scheduler::{JobScheduler, ScheduledJob, ScheduledJobRun, ScheduledJobState};

impl JobScheduler {
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
}

impl ScheduledJob {
    pub(super) fn push_recent_run(&mut self, run: ScheduledJobRun) {
        self.recent_runs
            .retain(|existing| existing.run_id != run.run_id);
        self.recent_runs.insert(0, run);
        self.recent_runs
            .sort_by(|left, right| right.run_id.cmp(&left.run_id));
        self.recent_runs.truncate(super::scheduler::MAX_RECENT_RUNS);
    }

    pub(super) fn transition_after_success(&mut self, now_ms: i64) {
        let is_one_shot = matches!(self.trigger, super::scheduler::ScheduledJobTrigger::At { .. });
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

    pub(super) fn transition_after_failure(&mut self, status: &str, error: &str, now_ms: i64) {
        let is_one_shot = matches!(self.trigger, super::scheduler::ScheduledJobTrigger::At { .. });
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
    pub(super) fn to_stored(&self, job_id: u64) -> StoredScheduledJobRun {
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
