use agentic_control_models::{
    ScheduleJobResult, ScheduledJobControlResult, ScheduledJobListResponse,
};
use agentic_protocol::OpCode;

use super::transport::{self, KernelBridge, KernelBridgeResult};

impl KernelBridge {
    pub fn list_scheduled_jobs(&mut self) -> KernelBridgeResult<ScheduledJobListResponse> {
        let response = self.send_control_command(OpCode::ListJobs, &[])?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::LIST_JOBS])
    }

    pub fn schedule_job(&mut self, payload: &str) -> KernelBridgeResult<ScheduleJobResult> {
        let response = self.send_control_command(OpCode::ScheduleJob, payload.as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::SCHEDULE_JOB])
    }

    pub fn set_job_enabled(
        &mut self,
        job_id: u64,
        enabled: bool,
    ) -> KernelBridgeResult<ScheduledJobControlResult> {
        let payload = serde_json::json!({ "job_id": job_id, "enabled": enabled });
        let response =
            self.send_control_command(OpCode::SetJobEnabled, payload.to_string().as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::SET_JOB_ENABLED],
        )
    }

    pub fn delete_job(&mut self, job_id: u64) -> KernelBridgeResult<ScheduledJobControlResult> {
        let payload = serde_json::json!({ "job_id": job_id });
        let response =
            self.send_control_command(OpCode::DeleteJob, payload.to_string().as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::DELETE_JOB])
    }
}
