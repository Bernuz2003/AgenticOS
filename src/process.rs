use candle_transformers::models::quantized_llama::ModelWeights;
use candle_transformers::generation::LogitsProcessor;

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessState {
    Ready,      // Appena creato, pronto a partire
    Running,    // In esecuzione (sta generando)
    Paused,     // In attesa (Context Switch)
    Finished,   // Ha finito di generare
}

pub struct AgentProcess {
    pub id: u64,
    pub state: ProcessState,
    
    // Il "Cervello" privato del processo (contiene la KV Cache)
    pub model: ModelWeights,
    // Il gestore della probabilità (temperatura, seed)
    pub logits_processor: LogitsProcessor,
    
    // Contesto di esecuzione
    pub tokens: Vec<u32>,     // Storico dei token generati
    pub index_pos: usize,     // A che punto siamo nella lettura/generazione
    pub max_tokens: usize,    // Limite imposto dall'utente
}

impl AgentProcess {
    pub fn new(id: u64, model: ModelWeights, prompt_tokens: Vec<u32>, max_gen: usize) -> Self {
        AgentProcess {
            id,
            state: ProcessState::Ready,
            model, // Questa istanza possiede la sua cache privata!
            logits_processor: LogitsProcessor::new(299792458 + id, Some(0.7), Some(0.9)), // Seed diverso per ogni agente
            tokens: prompt_tokens,
            index_pos: 0, // Inizierà elaborando il prompt (prefill)
            max_tokens: max_gen,
        }
    }

    pub fn is_finished(&self) -> bool {
        self.state == ProcessState::Finished
    }
}