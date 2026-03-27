mod lifecycle;
mod slot_manager;

use std::collections::HashMap;

use agentic_control_models::RemoteModelRuntimeView;
use tokenizers::Tokenizer;

use crate::backend::RuntimeModel;
use crate::model_catalog::ModelMetadata;
use crate::process::AgentProcess;
use crate::prompting::{GenerationConfig, PromptFamily};
use slot_manager::ResidentSlotManager;

pub struct LLMEngine {
    pub(super) master_model: Option<RuntimeModel>,
    pub(super) display_path: String,
    pub(super) runtime_reference: String,
    pub(super) backend_id: String,
    pub(super) driver_resolution_source: String,
    pub(super) driver_resolution_rationale: String,
    pub(super) loaded_remote_model: Option<RemoteModelRuntimeView>,
    pub tokenizer: Tokenizer,
    pub processes: HashMap<u64, AgentProcess>,
    pub(super) next_pid: u64,
    pub(super) family: PromptFamily,
    pub(super) metadata: Option<ModelMetadata>,
    pub(super) generation: GenerationConfig,
    pub(super) eos_token_id: u32,
    pub(super) eot_token_id: u32,
    pub(super) resident_slot_manager: ResidentSlotManager,
}
