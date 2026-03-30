use agentic_control_models::ScheduledJobView;

use crate::storage::{current_timestamp_ms, StorageService};

use super::scheduler::{JobScheduler, ScheduledJobState};

impl JobScheduler {
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

    pub fn scheduled_jobs(&self) -> Vec<&super::scheduler::ScheduledJob> {
        self.jobs.values().collect()
    }
}
