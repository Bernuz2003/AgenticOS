use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_llama::ModelWeights;

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
    pub model: ModelWeights,
    pub logits_processor: LogitsProcessor,
    pub tokens: Vec<u32>,
    pub index_pos: usize,
    pub max_tokens: usize,
}

impl AgentProcess {
    pub fn new(
        id: u64,
        owner_id: usize,
        model: ModelWeights,
        prompt_tokens: Vec<u32>,
        max_gen: usize,
    ) -> Self {
        AgentProcess {
            id,
            owner_id,
            state: ProcessState::Ready,
            model,
            logits_processor: LogitsProcessor::new(299792458 + id, Some(0.7), Some(0.9)),
            tokens: prompt_tokens,
            index_pos: 0,
            max_tokens: max_gen,
        }
    }
}
