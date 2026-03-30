use agentic_control_models::{LoadModelResult, ModelCatalogSnapshot, SelectModelResult};
use agentic_protocol::OpCode;

use super::transport::{self, KernelBridge, KernelBridgeResult};

impl KernelBridge {
    pub fn list_models(&mut self) -> KernelBridgeResult<ModelCatalogSnapshot> {
        let response = self.send_control_command(OpCode::ListModels, &[])?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::LIST_MODELS])
    }

    pub fn select_model(&mut self, model_id: &str) -> KernelBridgeResult<SelectModelResult> {
        let response = self.send_control_command(OpCode::SelectModel, model_id.as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::SELECT_MODEL])
    }

    pub fn load_model(&mut self, selector: &str) -> KernelBridgeResult<LoadModelResult> {
        let response = self.send_control_command(OpCode::Load, selector.as_bytes())?;
        if response.kind != "+OK" {
            self.drop_connection();
            return Err(transport::decode_protocol_error(
                &response.code,
                &response.payload,
            ));
        }

        self.decode_response(&response.payload, &[agentic_protocol::schema::LOAD])
    }
}
