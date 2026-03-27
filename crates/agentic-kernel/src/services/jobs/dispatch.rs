use crate::orchestrator::TaskGraphDef;
use crate::storage::{current_timestamp_ms, StorageService};

use super::scheduler::{DueJobDispatch, JobScheduler, ScheduledJobRun, ScheduledJobState};

impl JobScheduler {
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
}
