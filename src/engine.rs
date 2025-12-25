use anyhow::{Error as E, Result};
use candle_core::quantized::gguf_file;
use candle_core::{DType, Device, Tensor};
use candle_transformers::models::quantized_llama as model;
use model::ModelWeights;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokenizers::Tokenizer;

use crate::process::{AgentProcess, ProcessState};

/// Mantiene i dati del file caricati e gestisce i processi attivi.
pub struct LLMEngine {
    // Dati grezzi del file GGUF condivisi (per risparmiare I/O, anche se la RAM è usata dai pesi)
    file_content: Arc<Vec<u8>>,
    pub tokenizer: Tokenizer,
    device: Device,

    // Tabella dei Processi (PID -> Processo)
    processes: HashMap<u64, AgentProcess>,
    next_pid: u64,
}

impl LLMEngine {
    pub fn load(path: &str) -> Result<Self> {
        println!("ENGINE: Reading GGUF file into memory...");

        // 1. Leggiamo tutto il file in un buffer (Arc<Vec<u8>>)
        // Questo ci permette di creare cursori multipli per clonare il modello
        let bytes = std::fs::read(path)?;
        let file_content = Arc::new(bytes);

        // 2. Setup Tokenizer (come prima)
        let tokenizer_path = Path::new("models/tokenizer.json");
        let tokenizer = if tokenizer_path.exists() {
            Tokenizer::from_file(tokenizer_path).map_err(E::msg)?
        } else {
            let api = hf_hub::api::sync::Api::new()?;
            let repo = api.model("meta-llama/Meta-Llama-3-8B-Instruct".to_string());
            let path = repo.get("tokenizer.json")?;
            Tokenizer::from_file(path).map_err(E::msg)?
        };

        Ok(Self {
            file_content,
            tokenizer,
            device: Device::Cpu,
            processes: HashMap::new(),
            next_pid: 1,
        })
    }

    /// Crea un nuovo PROCESSO (Agente)
    /// Questo istanzia un nuovo ModelWeights (con la sua cache vuota)
    pub fn spawn_process(
        &mut self,
        prompt: &str,
        max_tokens: usize,
        owner_id: usize,
    ) -> Result<u64> {
        let pid = self.next_pid;
        self.next_pid += 1;

        println!("OS: Spawning Process PID {} for Owner {}", pid, owner_id);

        let mut cursor = Cursor::new(self.file_content.as_ref());
        let content = gguf_file::Content::read(&mut cursor)?;
        let model = ModelWeights::from_gguf(content, &mut cursor, &self.device)?;

        let tokens = self
            .tokenizer
            .encode(prompt, true)
            .map_err(E::msg)?
            .get_ids()
            .to_vec();

        // Passiamo owner_id al costruttore
        let process = AgentProcess::new(pid, owner_id, model, tokens, max_tokens);

        self.processes.insert(pid, process);
        Ok(pid)
    }

    /// Esegue UN PASSO (Step) di un processo specifico.
    /// Non blocca! Calcola 1 token e ritorna.
    pub fn step_process(&mut self, pid: u64) -> Result<Option<(String, usize)>> {
        let process = self
            .processes
            .get_mut(&pid)
            .ok_or(E::msg("PID not found"))?;

        if process.state == ProcessState::Finished {
            return Ok(None);
        }

        process.state = ProcessState::Running;
        let owner_id = process.owner_id; // Salviamo l'ID per ritornarlo

        let tokens = &mut process.tokens;
        let index_pos = process.index_pos;
        let context_size = if index_pos == 0 { tokens.len() } else { 1 };
        let start_pos = tokens.len().saturating_sub(context_size);
        let input_tokens = &tokens[start_pos..];
        let input_len = input_tokens.len();

        let input = Tensor::new(input_tokens, &self.device)?.unsqueeze(0)?;
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

        // Ritorniamo la tupla (Testo, OwnerID)
        if let Ok(t) = self.tokenizer.decode(&[next_token], true) {
            Ok(Some((t, owner_id)))
        } else {
            Ok(None)
        }
    }

    // Ritorna la lista dei PID attivi
    pub fn list_active_pids(&self) -> Vec<u64> {
        self.processes.keys().cloned().collect()
    }

    pub fn kill_process(&mut self, pid: u64) {
        self.processes.remove(&pid);
    }
}
