use anyhow::{Error as E, Result};
use candle_core::quantized::gguf_file;
use candle_core::{DType, Device, Tensor};
use candle_transformers::models::quantized_llama as model;
use model::ModelWeights;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokenizers::Tokenizer;

// Importiamo il modulo Process
use crate::process::{AgentProcess, ProcessState};

/// Mantiene il modello MASTER e gestisce i processi leggeri.
pub struct LLMEngine {
    // IL MASTER MODEL: Contiene i pesi condivisi (Arc)
    // Usiamo Option per poterlo inizializzare dopo se necessario, ma qui sarà sempre Some dopo load
    master_model: Option<ModelWeights>,

    pub tokenizer: Tokenizer,
    device: Device,

    // Tabella dei Processi (PID -> Processo)
    processes: HashMap<u64, AgentProcess>,
    next_pid: u64,
}

impl LLMEngine {
    /// Carica il modello GGUF UNA VOLTA SOLA.
    pub fn load(path: &str) -> Result<Self> {
        println!("ENGINE: Loading Master Model from {}...", path);

        let device = Device::Cpu;

        // 1. Caricamento Pesi (Pesante)
        let mut file = std::fs::File::open(path)
            .map_err(|e| E::msg(format!("Failed to open model file: {}", e)))?;
        let content = gguf_file::Content::read(&mut file)?;
        let model = ModelWeights::from_gguf(content, &mut file, &device)?;

        println!("ENGINE: Weights loaded. Loading Tokenizer...");

        // 2. Caricamento Tokenizer (Intelligente)
        // Cerca prima accanto al modello (es. models/tokenizer.json), poi nella root
        let model_path = Path::new(path);
        let parent_dir = model_path.parent().unwrap_or(Path::new("."));
        let local_tok_path = parent_dir.join("tokenizer.json");
        let root_tok_path = Path::new("tokenizer.json");

        let tokenizer = if local_tok_path.exists() {
            println!("ENGINE: Found tokenizer at {:?}", local_tok_path);
            Tokenizer::from_file(local_tok_path).map_err(E::msg)?
        } else if root_tok_path.exists() {
            println!("ENGINE: Found tokenizer at root (tokenizer.json)");
            Tokenizer::from_file(root_tok_path).map_err(E::msg)?
        } else {
            println!("ENGINE: Tokenizer not found locally. Downloading from HuggingFace (Meta-Llama-3-8B)...");
            println!("WARNING: This might hang if internet is slow!");
            let api = hf_hub::api::sync::Api::new()?;
            let repo = api.model("meta-llama/Meta-Llama-3-8B-Instruct".to_string());
            let path = repo.get("tokenizer.json")?;
            Tokenizer::from_file(path).map_err(E::msg)?
        };

        println!("ENGINE: Master Model & Tokenizer Ready. Zero-Copy Cloning enabled.");

        Ok(Self {
            master_model: Some(model),
            tokenizer,
            device,
            processes: HashMap::new(),
            next_pid: 1,
        })
    }

    /// Crea un nuovo PROCESSO (Agente) clonando il Master Model.
    /// Operazione O(1) in termini di memoria pesi.
    pub fn spawn_process(
        &mut self,
        prompt: &str,
        max_tokens: usize,
        owner_id: usize,
    ) -> Result<u64> {
        let pid = self.next_pid;
        self.next_pid += 1;

        println!(
            "OS: Forking Agent Process PID {} for Owner {} (Zero-Copy)",
            pid, owner_id
        );

        // 1. CLONAZIONE LEGGERA
        // Qui avviene la magia. Clone() sui tensori Candle incrementa solo il ref-count.
        // I pesi (GB) NON vengono copiati. La cache interna viene ricreata vuota.
        let model_clone = self
            .master_model
            .as_ref()
            .ok_or(E::msg("Master model not loaded"))?
            .clone();

        // 2. Tokenizziamo il prompt
        let tokens = self
            .tokenizer
            .encode(prompt, true)
            .map_err(E::msg)?
            .get_ids()
            .to_vec();

        // 3. Creiamo il processo con la sua copia privata (ma condivisa nei pesi) del modello
        let process = AgentProcess::new(pid, owner_id, model_clone, tokens, max_tokens);

        self.processes.insert(pid, process);

        Ok(pid)
    }

    /// Esegue UN PASSO (Step) di un processo specifico.
    pub fn step_process(&mut self, pid: u64) -> Result<Option<(String, usize)>> {
        let process = self
            .processes
            .get_mut(&pid)
            .ok_or(E::msg("PID not found"))?;

        if process.state == ProcessState::Finished {
            return Ok(None);
        }

        process.state = ProcessState::Running;
        let owner_id = process.owner_id;

        let tokens = &mut process.tokens;
        let index_pos = process.index_pos;

        let context_size = if index_pos == 0 { tokens.len() } else { 1 };
        let start_pos = tokens.len().saturating_sub(context_size);
        let input_tokens = &tokens[start_pos..];
        let input_len = input_tokens.len();

        let input = Tensor::new(input_tokens, &self.device)?.unsqueeze(0)?;

        // Forward pass sulla copia locale del modello (usa la cache locale automaticamente)
        let logits = process.model.forward(&input, index_pos)?;
        let logits = logits.squeeze(0)?.squeeze(0)?.to_dtype(DType::F32)?;

        let next_token = process.logits_processor.sample(&logits)?;
        tokens.push(next_token);

        process.index_pos += input_len;

        if next_token == 2
            || next_token == 128009
            || next_token == 128001
            || (tokens.len() - process.index_pos) >= process.max_tokens
        {
            process.state = ProcessState::Finished;
        }

        if let Ok(t) = self.tokenizer.decode(&[next_token], true) {
            Ok(Some((t, owner_id)))
        } else {
            Ok(None)
        }
    }

    pub fn list_active_pids(&self) -> Vec<u64> {
        self.processes.keys().cloned().collect()
    }

    pub fn kill_process(&mut self, pid: u64) {
        self.processes.remove(&pid);
    }
}
