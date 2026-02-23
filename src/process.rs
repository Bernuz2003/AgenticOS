use candle_transformers::generation::LogitsProcessor;

use crate::backend::RuntimeModel;
use crate::prompting::GenerationConfig;

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessState {
    Ready,
    Running,
    Paused,
    Finished,
}

pub struct AgentProcess {
    pub id: u64,
    pub owner_id: usize, // ID del socket proprietario
    pub state: ProcessState,
    pub model: RuntimeModel,
    pub logits_processor: LogitsProcessor,
    pub tokens: Vec<u32>,
    pub index_pos: usize,
    pub max_tokens: usize,
    pub syscall_buffer: String,
}

impl AgentProcess {
    pub fn new(
        id: u64,
        owner_id: usize,
        model: RuntimeModel,
        prompt_tokens: Vec<u32>,
        generation: GenerationConfig,
    ) -> Self {
        AgentProcess {
            id,
            owner_id,
            state: ProcessState::Ready,
            model,
            logits_processor: LogitsProcessor::new(
                generation.seed + id,
                Some(generation.temperature),
                Some(generation.top_p),
            ),
            tokens: prompt_tokens,
            index_pos: 0,
            max_tokens: generation.max_tokens,
            syscall_buffer: String::new(),
        }
    }
}
