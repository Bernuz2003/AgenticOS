use anyhow::{Error as E, Result};
use candle_core::quantized::gguf_file;
use candle_core::{DType, Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_llama as model;
use model::ModelWeights;
use std::io::Write;
use std::path::Path;
use tokenizers::Tokenizer; // Importante per il flush della console

pub struct LLMEngine {
    model: ModelWeights,
    tokenizer: Tokenizer,
    device: Device,
}

impl LLMEngine {
    /// Carica un modello GGUF dal disco
    pub fn load(path: &str) -> Result<Self> {
        println!("ENGINE: Loading model from {}...", path);

        // 1. Setup Device
        let device = Device::Cpu;

        // 2. Load Weights (Fix per Candle 0.9.1)
        let mut file = std::fs::File::open(path)?;
        let content = gguf_file::Content::read(&mut file)?;
        let model = ModelWeights::from_gguf(content, &mut file, &device)?;

        // 3. Load Tokenizer
        let tokenizer_path = Path::new("tokenizer.json");
        let tokenizer = if tokenizer_path.exists() {
            Tokenizer::from_file(tokenizer_path).map_err(E::msg)?
        } else {
            println!("ENGINE: tokenizer.json not found, fetching from HF...");
            let api = hf_hub::api::sync::Api::new()?;
            let repo = api.model("meta-llama/Meta-Llama-3-8B-Instruct".to_string());
            let path = repo.get("tokenizer.json")?;
            Tokenizer::from_file(path).map_err(E::msg)?
        };

        println!("ENGINE: Model & Tokenizer loaded successfully.");

        Ok(Self {
            model,
            tokenizer,
            device,
        })
    }

    /// Genera testo dato un prompt
    pub fn predict(&mut self, prompt: &str, max_tokens: usize) -> Result<String> {
        // 1. Tokenize
        let tokens = self.tokenizer.encode(prompt, true).map_err(E::msg)?;
        let mut tokens = tokens.get_ids().to_vec();
        let mut output_text = String::new();

        println!("ENGINE: Processing prompt ({} tokens)...", tokens.len());

        let mut logits_processor = LogitsProcessor::new(299792458, Some(0.7), Some(0.9));
        let mut index_pos = 0;

        for _i in 0..max_tokens {
            let context_size = if index_pos == 0 { tokens.len() } else { 1 };
            let start_pos = tokens.len().saturating_sub(context_size);

            // --- FIX BORROW CHECKER ---
            // Prendiamo la slice ma salviamo subito la lunghezza in un intero copiato
            let input_tokens = &tokens[start_pos..];
            let input_len = input_tokens.len();

            let input = Tensor::new(input_tokens, &self.device)?.unsqueeze(0)?;
            // Qui 'input_tokens' non serve più, quindi il borrow finisce

            // Forward pass
            let logits = self.model.forward(&input, index_pos)?;
            let logits = logits.squeeze(0)?.squeeze(0)?.to_dtype(DType::F32)?;

            // Sampling
            let next_token = logits_processor.sample(&logits)?;

            // Ora possiamo modificare 'tokens' perché non abbiamo riferimenti aperti
            tokens.push(next_token);

            // Decode & Streaming Print
            if let Ok(t) = self.tokenizer.decode(&[next_token], true) {
                eprint!("{}", t);
                // Forziamo il flush sia di stdout che stderr per sicurezza
                use std::io::Write;
                let _ = std::io::stdout().flush();
                let _ = std::io::stderr().flush();

                output_text.push_str(&t);
            }

            // Usiamo la variabile copiata 'input_len'
            index_pos += input_len;

            // Stop tokens (2 = TinyLlama EOS, 128001/9 = Llama 3 EOS)
            if next_token == 2 || next_token == 128009 || next_token == 128001 {
                println!("\nENGINE: Stop token hit!");
                break;
            }
        }

        println!(""); // A capo finale
        Ok(output_text)
    }
}
