use anyhow::{Error as E, Result};
use candle_core::{DType, Device, Tensor};
use std::path::PathBuf;

use crate::backend::RuntimeModel;
use crate::process::{AgentProcess, ProcessState};
use crate::prompting::{should_stop_on_text, GenerationConfig};

use super::tokenizer::{resolve_special_tokens, resolve_tokenizer_path};
use super::LLMEngine;

impl LLMEngine {
    pub fn load(path: &str, family: crate::prompting::PromptFamily, tokenizer_hint: Option<PathBuf>) -> Result<Self> {
        println!("ENGINE: Loading Master Model from {}...", path);

        let device = Device::Cpu;
        let model = RuntimeModel::load_from_gguf(path, family, &device)?;

        println!("ENGINE: Weights loaded. Loading Tokenizer...");

        let tokenizer_path = resolve_tokenizer_path(path, tokenizer_hint)
            .ok_or_else(|| E::msg("Tokenizer not found for selected model (fail-fast policy)."))?;
        println!("ENGINE: Using tokenizer at {:?}", tokenizer_path);
        let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path).map_err(E::msg)?;

        let (eos_token_id, eot_token_id) = resolve_special_tokens(&tokenizer, family).map_err(E::msg)?;

        println!(
            "ENGINE: Special Tokens Identified -> EOS: {}, EOT: {}",
            eos_token_id, eot_token_id
        );
        println!(
            "ENGINE: Master Model & Tokenizer Ready. Backend abstraction enabled."
        );

        Ok(Self {
            master_model: Some(model),
            model_path: path.to_string(),
            tokenizer,
            device,
            processes: std::collections::HashMap::new(),
            next_pid: 1,
            family,
            generation: GenerationConfig::defaults_for(family),
            eos_token_id,
            eot_token_id,
        })
    }

    pub fn spawn_process(
        &mut self,
        prompt: &str,
        _max_tokens: usize,
        owner_id: usize,
    ) -> Result<u64> {
        let pid = self.next_pid;
        self.next_pid += 1;

        println!(
            "OS: Forking Agent Process PID {} for Owner {} (Zero-Copy)",
            pid, owner_id
        );

        let model_clone = {
            let master = self
                .master_model
                .as_ref()
                .ok_or(E::msg("Master model not loaded"))?;

            if let Some(dup) = master.duplicate_if_supported() {
                dup
            } else {
                println!(
                    "ENGINE: Runtime backend for {:?} is not cloneable; spawning PID {} by reloading model instance.",
                    self.family, pid
                );
                RuntimeModel::load_from_gguf(&self.model_path, self.family, &self.device)?
            }
        };

        let tokens = self
            .tokenizer
            .encode(prompt, true)
            .map_err(E::msg)?
            .get_ids()
            .to_vec();

        let process = AgentProcess::new(pid, owner_id, model_clone, tokens, self.generation);
        self.processes.insert(pid, process);

        Ok(pid)
    }

    pub fn step_process(&mut self, pid: u64) -> Result<Option<(String, usize)>> {
        let process = self
            .processes
            .get_mut(&pid)
            .ok_or(E::msg("PID not found"))?;

        if process.state == ProcessState::Finished
            || process.state == ProcessState::WaitingForMemory
            || process.state == ProcessState::Paused
        {
            return Ok(None);
        }

        process.state = ProcessState::Running;
        let owner_id = process.owner_id;

        let mut next_token = 0;

        while process.index_pos < process.tokens.len() {
            let input_token = process.tokens[process.index_pos];
            let input_tensor = Tensor::new(&[input_token], &self.device)?.unsqueeze(0)?;
            let logits = process.model.forward(&input_tensor, process.index_pos)?;
            process.index_pos += 1;

            if process.index_pos == process.tokens.len() {
                let logits = logits.squeeze(0)?.squeeze(0)?.to_dtype(DType::F32)?;
                next_token = process.logits_processor.sample(&logits)?;
            }
        }

        process.tokens.push(next_token);
        let text_output = self.tokenizer.decode(&[next_token], true).ok();

        let mut should_stop = false;
        if next_token == self.eos_token_id || next_token == self.eot_token_id || next_token == 2 {
            should_stop = true;
        }

        if let Some(ref t) = text_output {
            if should_stop_on_text(self.family, t) {
                should_stop = true;
            }
        }

        if process.tokens.len() >= process.max_tokens {
            should_stop = true;
        }

        if should_stop {
            process.state = ProcessState::Finished;
        }

        if let Some(t) = text_output {
            Ok(Some((t, owner_id)))
        } else {
            Ok(None)
        }
    }

    pub fn inject_context(&mut self, pid: u64, text: &str) -> Result<()> {
        let process = self
            .processes
            .get_mut(&pid)
            .ok_or(E::msg("PID not found"))?;

        let formatted_text = format!("\n{}\n", text);

        let new_tokens = self
            .tokenizer
            .encode(formatted_text.as_str(), true)
            .map_err(E::msg)?
            .get_ids()
            .to_vec();

        println!("OS: Injecting {} tokens into PID {}", new_tokens.len(), pid);
        process.tokens.extend(new_tokens);
        process.state = ProcessState::Running;
        Ok(())
    }

    pub fn list_active_pids(&self) -> Vec<u64> {
        self.processes
            .iter()
            .filter_map(|(pid, proc)| {
                if proc.state != ProcessState::Finished {
                    Some(*pid)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn list_waiting_pids(&self) -> Vec<u64> {
        self.processes
            .iter()
            .filter_map(|(pid, proc)| {
                if proc.state == ProcessState::WaitingForMemory {
                    Some(*pid)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn process_owner_id(&self, pid: u64) -> Option<usize> {
        self.processes.get(&pid).map(|p| p.owner_id)
    }

    pub fn list_finished_pids(&self) -> Vec<u64> {
        self.processes
            .iter()
            .filter_map(|(pid, process)| {
                if process.state == ProcessState::Finished {
                    Some(*pid)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn kill_process(&mut self, pid: u64) {
        self.processes.remove(&pid);
    }

    pub fn terminate_process(&mut self, pid: u64) -> bool {
        if let Some(proc) = self.processes.get_mut(&pid) {
            proc.state = ProcessState::Finished;
            true
        } else {
            false
        }
    }

    pub fn set_process_waiting_for_memory(&mut self, pid: u64) -> bool {
        if let Some(proc) = self.processes.get_mut(&pid) {
            if proc.state != ProcessState::Finished {
                proc.state = ProcessState::WaitingForMemory;
                return true;
            }
        }
        false
    }

    pub fn set_process_ready_if_waiting(&mut self, pid: u64) -> bool {
        if let Some(proc) = self.processes.get_mut(&pid) {
            if proc.state == ProcessState::WaitingForMemory {
                proc.state = ProcessState::Ready;
                return true;
            }
        }
        false
    }

    pub fn process_status_line(&self, pid: u64) -> Option<String> {
        self.processes.get(&pid).map(|p| {
            format!(
                "pid={} owner_id={} state={:?} tokens={} index_pos={} max_tokens={}",
                pid,
                p.owner_id,
                p.state,
                p.tokens.len(),
                p.index_pos,
                p.max_tokens
            )
        })
    }

    pub fn process_max_tokens(&self, pid: u64) -> Option<usize> {
        self.processes.get(&pid).map(|p| p.max_tokens)
    }

    pub fn set_generation_config(&mut self, cfg: GenerationConfig) {
        self.generation = cfg;
    }

    pub fn generation_config(&self) -> GenerationConfig {
        self.generation
    }
}
