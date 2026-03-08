mod lifecycle;
mod tokenizer;

use std::collections::HashMap;

use candle_core::Device;
use tokenizers::Tokenizer;

use crate::backend::RuntimeModel;
use crate::model_catalog::ModelMetadata;
use crate::process::AgentProcess;
use crate::prompting::{GenerationConfig, PromptFamily};

pub struct LLMEngine {
    pub(super) master_model: Option<RuntimeModel>,
    pub(super) model_path: String,
    pub(super) backend_id: String,
    pub(super) driver_resolution_source: String,
    pub(super) driver_resolution_rationale: String,
    pub tokenizer: Tokenizer,
    pub(super) device: Device,
    pub processes: HashMap<u64, AgentProcess>,
    pub(super) next_pid: u64,
    pub(super) family: PromptFamily,
    pub(super) metadata: Option<ModelMetadata>,
    pub(super) generation: GenerationConfig,
    pub(super) eos_token_id: u32,
    pub(super) eot_token_id: u32,
}
