use rusqlite::{params, OptionalExtension};

use super::service::{StorageError, StorageService};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewScheduledJobRecord {
    pub name: String,
    pub target_kind: String,
    pub workflow_payload: String,
    pub trigger_kind: String,
    pub trigger_payload: String,
    pub timeout_ms: u64,
    pub max_retries: u32,
    pub backoff_ms: u64,
    pub enabled: bool,
    pub state: String,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredScheduledJob {
    pub job_id: u64,
    pub name: String,
    pub target_kind: String,
    pub workflow_payload: String,
    pub trigger_kind: String,
    pub trigger_payload: String,
    pub timeout_ms: u64,
    pub max_retries: u32,
    pub backoff_ms: u64,
    pub enabled: bool,
    pub state: String,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredScheduledJobRun {
    pub run_id: u64,
    pub job_id: u64,
    pub trigger_at_ms: i64,
    pub attempt: u32,
    pub status: String,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: Option<i64>,
    pub orchestration_id: Option<u64>,
    pub deadline_at_ms: Option<i64>,
    pub error: Option<String>,
}

impl StorageService {
    pub(crate) fn insert_scheduled_job(
        &mut self,
        new_job: &NewScheduledJobRecord,
    ) -> Result<StoredScheduledJob, StorageError> {
        self.connection.execute(
            r#"
            INSERT INTO scheduled_jobs (
                name,
                target_kind,
                workflow_payload,
                trigger_kind,
                trigger_payload,
                timeout_ms,
                max_retries,
                backoff_ms,
                enabled,
                state,
                next_run_at_ms,
                current_trigger_at_ms,
                current_attempt,
                active_run_id,
                active_orchestration_id,
                active_deadline_at_ms,
                last_run_started_at_ms,
                last_run_completed_at_ms,
                last_run_status,
                last_error,
                consecutive_failures,
                created_at_ms,
                updated_at_ms
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                ?19, ?20, ?21, ?22, ?23
            )
            "#,
            params![
                new_job.name,
                new_job.target_kind,
                new_job.workflow_payload,
                new_job.trigger_kind,
                new_job.trigger_payload,
                new_job.timeout_ms as i64,
                new_job.max_retries as i64,
                new_job.backoff_ms as i64,
                bool_to_int(new_job.enabled),
                new_job.state,
                new_job.next_run_at_ms,
                new_job.current_trigger_at_ms,
                new_job.current_attempt as i64,
                new_job.active_run_id,
                new_job.active_orchestration_id,
                new_job.active_deadline_at_ms,
                new_job.last_run_started_at_ms,
                new_job.last_run_completed_at_ms,
                new_job.last_run_status,
                new_job.last_error,
                new_job.consecutive_failures as i64,
                new_job.created_at_ms,
                new_job.updated_at_ms,
            ],
        )?;

        let job_id = self.connection.last_insert_rowid() as u64;
        self.scheduled_job_by_id(job_id)?
            .ok_or_else(|| StorageError::Sqlite(rusqlite::Error::QueryReturnedNoRows))
    }

    pub(crate) fn save_scheduled_job(
        &mut self,
        job: &StoredScheduledJob,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            r#"
            UPDATE scheduled_jobs
            SET
                name = ?2,
                target_kind = ?3,
                workflow_payload = ?4,
                trigger_kind = ?5,
                trigger_payload = ?6,
                timeout_ms = ?7,
                max_retries = ?8,
                backoff_ms = ?9,
                enabled = ?10,
                state = ?11,
                next_run_at_ms = ?12,
                current_trigger_at_ms = ?13,
                current_attempt = ?14,
                active_run_id = ?15,
                active_orchestration_id = ?16,
                active_deadline_at_ms = ?17,
                last_run_started_at_ms = ?18,
                last_run_completed_at_ms = ?19,
                last_run_status = ?20,
                last_error = ?21,
                consecutive_failures = ?22,
                created_at_ms = ?23,
                updated_at_ms = ?24
            WHERE job_id = ?1
            "#,
            params![
                job.job_id,
                job.name,
                job.target_kind,
                job.workflow_payload,
                job.trigger_kind,
                job.trigger_payload,
                job.timeout_ms as i64,
                job.max_retries as i64,
                job.backoff_ms as i64,
                bool_to_int(job.enabled),
                job.state,
                job.next_run_at_ms,
                job.current_trigger_at_ms,
                job.current_attempt as i64,
                job.active_run_id,
                job.active_orchestration_id,
                job.active_deadline_at_ms,
                job.last_run_started_at_ms,
                job.last_run_completed_at_ms,
                job.last_run_status,
                job.last_error,
                job.consecutive_failures as i64,
                job.created_at_ms,
                job.updated_at_ms,
            ],
        )?;
        Ok(())
    }

    pub(crate) fn delete_scheduled_job(&mut self, job_id: u64) -> Result<(), StorageError> {
        self.connection.execute(
            "DELETE FROM scheduled_jobs WHERE job_id = ?1",
            params![job_id],
        )?;
        Ok(())
    }

    pub(crate) fn load_scheduled_jobs(&self) -> Result<Vec<StoredScheduledJob>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                job_id,
                name,
                target_kind,
                workflow_payload,
                trigger_kind,
                trigger_payload,
                timeout_ms,
                max_retries,
                backoff_ms,
                enabled,
                state,
                next_run_at_ms,
                current_trigger_at_ms,
                current_attempt,
                active_run_id,
                active_orchestration_id,
                active_deadline_at_ms,
                last_run_started_at_ms,
                last_run_completed_at_ms,
                last_run_status,
                last_error,
                consecutive_failures,
                created_at_ms,
                updated_at_ms
            FROM scheduled_jobs
            ORDER BY created_at_ms ASC, job_id ASC
            "#,
        )?;
        let rows = statement.query_map([], map_scheduled_job_row)?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    }

    pub(crate) fn load_scheduled_job_runs(
        &self,
        job_id: u64,
        limit: usize,
    ) -> Result<Vec<StoredScheduledJobRun>, StorageError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT
                run_id,
                job_id,
                trigger_at_ms,
                attempt,
                status,
                started_at_ms,
                completed_at_ms,
                orchestration_id,
                deadline_at_ms,
                error
            FROM scheduled_job_runs
            WHERE job_id = ?1
            ORDER BY run_id DESC
            LIMIT ?2
            "#,
        )?;
        let rows = statement.query_map(params![job_id, limit as i64], map_scheduled_job_run_row)?;

        let mut runs = Vec::new();
        for row in rows {
            runs.push(row?);
        }
        Ok(runs)
    }

    pub(crate) fn insert_scheduled_job_run(
        &mut self,
        run: &StoredScheduledJobRun,
    ) -> Result<u64, StorageError> {
        self.connection.execute(
            r#"
            INSERT INTO scheduled_job_runs (
                job_id,
                trigger_at_ms,
                attempt,
                status,
                started_at_ms,
                completed_at_ms,
                orchestration_id,
                deadline_at_ms,
                error
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                run.job_id,
                run.trigger_at_ms,
                run.attempt as i64,
                run.status,
                run.started_at_ms,
                run.completed_at_ms,
                run.orchestration_id,
                run.deadline_at_ms,
                run.error,
            ],
        )?;
        Ok(self.connection.last_insert_rowid() as u64)
    }

    pub(crate) fn save_scheduled_job_run(
        &mut self,
        run: &StoredScheduledJobRun,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            r#"
            UPDATE scheduled_job_runs
            SET
                job_id = ?2,
                trigger_at_ms = ?3,
                attempt = ?4,
                status = ?5,
                started_at_ms = ?6,
                completed_at_ms = ?7,
                orchestration_id = ?8,
                deadline_at_ms = ?9,
                error = ?10
            WHERE run_id = ?1
            "#,
            params![
                run.run_id,
                run.job_id,
                run.trigger_at_ms,
                run.attempt as i64,
                run.status,
                run.started_at_ms,
                run.completed_at_ms,
                run.orchestration_id,
                run.deadline_at_ms,
                run.error,
            ],
        )?;
        Ok(())
    }

    pub(crate) fn scheduled_job_by_id(
        &self,
        job_id: u64,
    ) -> Result<Option<StoredScheduledJob>, StorageError> {
        Ok(self
            .connection
            .query_row(
                r#"
                SELECT
                    job_id,
                    name,
                    target_kind,
                    workflow_payload,
                    trigger_kind,
                    trigger_payload,
                    timeout_ms,
                    max_retries,
                    backoff_ms,
                    enabled,
                    state,
                    next_run_at_ms,
                    current_trigger_at_ms,
                    current_attempt,
                    active_run_id,
                    active_orchestration_id,
                    active_deadline_at_ms,
                    last_run_started_at_ms,
                    last_run_completed_at_ms,
                    last_run_status,
                    last_error,
                    consecutive_failures,
                    created_at_ms,
                    updated_at_ms
                FROM scheduled_jobs
                WHERE job_id = ?1
                "#,
                params![job_id],
                map_scheduled_job_row,
            )
            .optional()?)
    }
}

fn map_scheduled_job_row(row: &rusqlite::Row<'_>) -> Result<StoredScheduledJob, rusqlite::Error> {
    Ok(StoredScheduledJob {
        job_id: row.get(0)?,
        name: row.get(1)?,
        target_kind: row.get(2)?,
        workflow_payload: row.get(3)?,
        trigger_kind: row.get(4)?,
        trigger_payload: row.get(5)?,
        timeout_ms: row.get::<_, i64>(6)?.max(0) as u64,
        max_retries: row.get::<_, i64>(7)?.max(0) as u32,
        backoff_ms: row.get::<_, i64>(8)?.max(0) as u64,
        enabled: row.get::<_, i64>(9)? != 0,
        state: row.get(10)?,
        next_run_at_ms: row.get(11)?,
        current_trigger_at_ms: row.get(12)?,
        current_attempt: row.get::<_, i64>(13)?.max(0) as u32,
        active_run_id: row.get(14)?,
        active_orchestration_id: row.get(15)?,
        active_deadline_at_ms: row.get(16)?,
        last_run_started_at_ms: row.get(17)?,
        last_run_completed_at_ms: row.get(18)?,
        last_run_status: row.get(19)?,
        last_error: row.get(20)?,
        consecutive_failures: row.get::<_, i64>(21)?.max(0) as u32,
        created_at_ms: row.get(22)?,
        updated_at_ms: row.get(23)?,
    })
}

fn map_scheduled_job_run_row(
    row: &rusqlite::Row<'_>,
) -> Result<StoredScheduledJobRun, rusqlite::Error> {
    Ok(StoredScheduledJobRun {
        run_id: row.get(0)?,
        job_id: row.get(1)?,
        trigger_at_ms: row.get(2)?,
        attempt: row.get::<_, i64>(3)?.max(0) as u32,
        status: row.get(4)?,
        started_at_ms: row.get(5)?,
        completed_at_ms: row.get(6)?,
        orchestration_id: row.get(7)?,
        deadline_at_ms: row.get(8)?,
        error: row.get(9)?,
    })
}

fn bool_to_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}
