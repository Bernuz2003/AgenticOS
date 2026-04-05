use agentic_control_models::{
    CoreDumpInfoRequest, CoreDumpInfoResponse, CoreDumpListRequest, CoreDumpListResponse,
    CoreDumpReplayRequest, CoreDumpReplayResult, CoreDumpRequest, CoreDumpSummaryView,
};
use agentic_protocol::OpCode;

use super::transport::{self, KernelBridge, KernelBridgeResult};

impl KernelBridge {
    pub fn capture_core_dump(
        &mut self,
        pid: Option<u64>,
        session_id: Option<&str>,
        reason: Option<&str>,
        note: Option<&str>,
    ) -> KernelBridgeResult<CoreDumpSummaryView> {
        let payload = serde_json::to_vec(&CoreDumpRequest {
            pid,
            session_id: session_id.map(ToOwned::to_owned),
            mode: Some("manual".to_string()),
            reason: reason.map(ToOwned::to_owned),
            include_workspace: None,
            include_backend_state: None,
            freeze_target: Some(false),
            note: note.map(ToOwned::to_owned),
        })?;
        let response = self.send_control_command(OpCode::CoreDump, &payload)?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response::<agentic_control_models::CoreDumpCaptureResult>(
            &response.payload,
            &[agentic_protocol::schema::COREDUMP],
        )
        .map(|result| result.dump)
    }

    pub fn list_core_dumps(
        &mut self,
        limit: Option<usize>,
    ) -> KernelBridgeResult<CoreDumpListResponse> {
        let payload = serde_json::to_vec(&CoreDumpListRequest { limit })?;
        let response = self.send_control_command(OpCode::ListCoreDumps, &payload)?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::LIST_COREDUMPS],
        )
    }

    pub fn fetch_core_dump_info(
        &mut self,
        dump_id: &str,
    ) -> KernelBridgeResult<CoreDumpInfoResponse> {
        let payload = serde_json::to_vec(&CoreDumpInfoRequest {
            dump_id: dump_id.to_string(),
        })?;
        let response = self.send_control_command(OpCode::CoreDumpInfo, &payload)?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::COREDUMP_INFO],
        )
    }

    pub fn replay_core_dump(
        &mut self,
        dump_id: &str,
        branch_label: Option<&str>,
    ) -> KernelBridgeResult<CoreDumpReplayResult> {
        let payload = serde_json::to_vec(&CoreDumpReplayRequest {
            dump_id: dump_id.to_string(),
            branch_label: branch_label.map(ToOwned::to_owned),
            tool_mode: None,
            patch: None,
        })?;
        let response = self.send_control_command(OpCode::ReplayCoreDump, &payload)?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(
            &response.payload,
            &[agentic_protocol::schema::REPLAY_COREDUMP],
        )
    }
}
