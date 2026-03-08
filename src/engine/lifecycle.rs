use anyhow::{Error as E, Result};
use candle_core::Device;
use std::path::PathBuf;

use crate::backend::{DriverResolution, RuntimeModel};
use crate::memory::ContextSlotId;
use crate::model_catalog::{ModelMetadata, ResolvedModelTarget};
use crate::process::{AgentProcess, ProcessState};
use crate::prompting::{
    format_interprocess_user_message_with_metadata,
    format_system_injection_with_metadata,
    format_user_message_with_metadata,
    should_stop_on_text_with_metadata,
    GenerationConfig,
};

use super::tokenizer::{resolve_special_tokens, resolve_tokenizer_path};
use super::LLMEngine;

impl LLMEngine {
    pub fn load_target(target: &ResolvedModelTarget) -> Result<Self> {
        Self::load(
            target.path.to_string_lossy().as_ref(),
            target.family,
            target.tokenizer_path.clone(),
            target.metadata.clone(),
            target.driver_resolution.clone(),
        )
    }

    fn load(
        path: &str,
        family: crate::prompting::PromptFamily,
        tokenizer_hint: Option<PathBuf>,
        metadata: Option<ModelMetadata>,
        driver_resolution: DriverResolution,
    ) -> Result<Self> {
        tracing::info!(path, ?family, "ENGINE: Loading Master Model");

        let device = Device::Cpu;
        let model = RuntimeModel::load_from_gguf(
            path,
            family,
            &driver_resolution.resolved_backend_id,
            &device,
        )?;
        let resolved_family = model.family();
        let backend_id = model.backend_id();

        tracing::info!(
            backend_id,
            resolution_source = driver_resolution.resolution_source,
            resolution_rationale = driver_resolution.resolution_rationale,
            ?resolved_family,
            "ENGINE: Weights loaded. Loading Tokenizer..."
        );

        let tokenizer_path = resolve_tokenizer_path(path, tokenizer_hint)
            .ok_or_else(|| E::msg("Tokenizer not found for selected model (fail-fast policy)."))?;
        tracing::info!(?tokenizer_path, "ENGINE: Using tokenizer");
        let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path).map_err(E::msg)?;

        let (eos_token_id, eot_token_id) = resolve_special_tokens(
            &tokenizer,
            resolved_family,
            metadata.as_ref(),
        )
        .map_err(E::msg)?;

        tracing::info!(
            eos_token_id,
            eot_token_id,
            "ENGINE: Special Tokens Identified"
        );
        tracing::info!("ENGINE: Master Model & Tokenizer Ready. Backend abstraction enabled.");

        Ok(Self {
            master_model: Some(model),
            model_path: path.to_string(),
            backend_id: backend_id.to_string(),
            driver_resolution_source: driver_resolution.resolution_source.to_string(),
            driver_resolution_rationale: driver_resolution.resolution_rationale,
            tokenizer,
            device,
            processes: std::collections::HashMap::new(),
            next_pid: 1,
            family: resolved_family,
            metadata,
            generation: GenerationConfig::defaults_for(resolved_family),
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

        tracing::info!(
            pid,
            owner_id,
            "OS: Forking Agent Process (Zero-Copy)"
        );

        let model_clone = {
            let master = self
                .master_model
                .as_ref()
                .ok_or(E::msg("Master model not loaded"))?;

            if let Some(dup) = master.duplicate_if_supported() {
                dup
            } else if self.processes.is_empty() {
                // First process for a non-cloneable backend (e.g. Qwen2):
                // reload is acceptable because there's nothing else running.
                tracing::info!(
                    family = ?self.family,
                    backend_id = self.backend_id,
                    pid,
                    "ENGINE: Runtime backend not cloneable; reloading model instance (first process)"
                );
                RuntimeModel::load_from_gguf(
                    &self.model_path,
                    self.family,
                    &self.backend_id,
                    &self.device,
                )?
            } else {
                // C7 guard: reject concurrent spawn for non-cloneable backends.
                return Err(E::msg(format!(
                    "Cannot spawn additional process: {:?} backend does not support model cloning. \
                     Terminate existing processes first (active PIDs: {:?}).",
                    self.family,
                    self.processes.keys().collect::<Vec<_>>()
                )));
            }
        };

        let formatted_prompt = format_user_message_with_metadata(
            prompt,
            self.family,
            self.metadata.as_ref(),
        );

        let tokens = self
            .tokenizer
            .encode(formatted_prompt, true)
            .map_err(E::msg)?
            .get_ids()
            .to_vec();

        let process = AgentProcess::new(
            pid,
            owner_id,
            model_clone,
            self.tokenizer.clone(),
            tokens,
            self.generation,
        );
        self.processes.insert(pid, process);

        Ok(pid)
    }

    pub fn set_process_context_slot(&mut self, pid: u64, slot_id: ContextSlotId) -> Result<()> {
        let process = self
            .processes
            .get_mut(&pid)
            .ok_or(E::msg("PID not found"))?;
        process.context_slot_id = Some(slot_id);
        Ok(())
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

        let step = process.model.generate_step(
            process.context_slot_id,
            &process.tokens,
            process.index_pos,
            &mut process.logits_processor,
            &process.tokenizer,
            process.generation,
            &self.device,
            self.eos_token_id,
            self.eot_token_id,
        )?;

        process.index_pos = step.next_index_pos;
        process.tokens.extend(step.appended_tokens);
        let text_output = if step.emitted_text.is_empty() {
            None
        } else {
            Some(step.emitted_text)
        };

        let mut should_stop = step.finished;

        if let Some(ref t) = text_output {
            if should_stop_on_text_with_metadata(self.family, t, self.metadata.as_ref()) {
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

        tracing::debug!(pid, tokens = new_tokens.len(), "OS: Injecting tokens");
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

    pub fn loaded_model_path(&self) -> &str {
        &self.model_path
    }

    pub fn loaded_backend_id(&self) -> &str {
        &self.backend_id
    }

    pub fn driver_resolution_source(&self) -> &str {
        &self.driver_resolution_source
    }

    pub fn driver_resolution_rationale(&self) -> &str {
        &self.driver_resolution_rationale
    }

    pub fn loaded_family(&self) -> crate::prompting::PromptFamily {
        self.family
    }

    pub fn model_metadata(&self) -> Option<&ModelMetadata> {
        self.metadata.as_ref()
    }

    pub fn free_context_slot(&mut self, slot_id: ContextSlotId) -> Result<()> {
        let model = self
            .master_model
            .as_mut()
            .ok_or_else(|| E::msg("Master model not loaded"))?;
        model.free_context_slot(slot_id)
    }

    pub fn format_system_message(&self, content: &str) -> String {
        format_system_injection_with_metadata(content, self.family, self.metadata.as_ref())
    }

    pub fn format_interprocess_message(&self, from_pid: u64, message: &str) -> String {
        format_interprocess_user_message_with_metadata(
            from_pid,
            message,
            self.family,
            self.metadata.as_ref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::model_catalog::ModelCatalog;
    use crate::prompting::PromptFamily;

    #[test]
    #[ignore = "uses the real local Qwen3.5 artifacts to validate generic discovery and rejection"]
    fn qwen35_catalog_target_is_rejected_before_backend_load() {
        let model_id = "qwen3.5-9b/Qwen3.5-9B-Q4_K_M";
        let catalog = ModelCatalog::discover("models").expect("discover models");
        let entry = catalog.find_by_id(model_id).expect("qwen3.5 entry present");

        assert_eq!(entry.family, PromptFamily::Qwen);
        assert!(
            entry.tokenizer_path.is_some(),
            "qwen3.5 tokenizer must be discoverable"
        );
        assert_eq!(
            entry.metadata.as_ref().and_then(|meta| meta.architecture.as_deref()),
            Some("qwen35")
        );

        let err = catalog
            .resolve_load_target(model_id)
            .expect_err("qwen3.5 should fail generic driver resolution until a compatible backend exists");
        assert!(err.to_string().contains("qwen35"));
    }
}
