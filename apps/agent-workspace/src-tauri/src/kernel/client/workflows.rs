use agentic_control_models::{
    ArtifactListRequest, ArtifactListResponse, OrchStatusResponse, OrchestrateResult,
    OrchestrationControlResult, OrchestrationListResponse, OrchestrationStatusRequest,
    RetryTaskResult,
};
use agentic_protocol::OpCode;

use super::transport::{self, KernelBridge, KernelBridgeResult};

impl KernelBridge {
    pub fn fetch_orchestration_status(
        &mut self,
        orch_id: u64,
    ) -> KernelBridgeResult<OrchStatusResponse> {
        let payload = serde_json::to_vec(&OrchestrationStatusRequest {
            orchestration_id: orch_id,
        })?;
        let response = self.send_control_command(OpCode::OrchestrationStatus, &payload)?;

        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::ORCHESTRATION_STATUS],
        )
    }

    pub fn list_orchestrations(&mut self) -> KernelBridgeResult<OrchestrationListResponse> {
        let response = self.send_control_command(OpCode::ListOrchestrations, &[])?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::LIST_ORCHESTRATIONS],
        )
    }

    pub fn list_artifacts(
        &mut self,
        orchestration_id: u64,
        task: Option<&str>,
    ) -> KernelBridgeResult<ArtifactListResponse> {
        let payload = serde_json::to_vec(&ArtifactListRequest {
            orchestration_id,
            task: task.map(ToOwned::to_owned),
        })?;
        let response = self.send_control_command(OpCode::ListArtifacts, &payload)?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::LIST_ARTIFACTS],
        )
    }

    pub fn orchestrate(&mut self, payload: &str) -> KernelBridgeResult<OrchestrateResult> {
        let response = self.send_control_command(OpCode::Orchestrate, payload.as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::ORCHESTRATE])
    }

    pub fn retry_task(
        &mut self,
        orch_id: u64,
        task_id: &str,
    ) -> KernelBridgeResult<RetryTaskResult> {
        let payload = serde_json::json!({
            "orchestration_id": orch_id,
            "task_id": task_id,
        });
        let response =
            self.send_control_command(OpCode::RetryTask, payload.to_string().as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::RETRY_TASK])
    }

    pub fn stop_orchestration(
        &mut self,
        orch_id: u64,
    ) -> KernelBridgeResult<OrchestrationControlResult> {
        let payload = serde_json::json!({ "orchestration_id": orch_id });
        let response =
            self.send_control_command(OpCode::StopOrchestration, payload.to_string().as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::STOP_ORCHESTRATION],
        )
    }

    pub fn delete_orchestration(
        &mut self,
        orch_id: u64,
    ) -> KernelBridgeResult<OrchestrationControlResult> {
        let payload = serde_json::json!({ "orchestration_id": orch_id });
        let response =
            self.send_control_command(OpCode::DeleteOrchestration, payload.to_string().as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::DELETE_ORCHESTRATION],
        )
    }
}
