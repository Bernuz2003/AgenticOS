use anyhow::{Error as E, Result};
use std::path::PathBuf;

use crate::backend::{DriverResolution, RuntimeModel};
use crate::memory::ContextSlotId;
use crate::model_catalog::{ModelMetadata, ResolvedModelTarget};
use crate::process::{
    AgentProcess, ContextPolicy, InitialContextSeed, ProcessLifecyclePolicy, ProcessState,
    ResidentSlotPolicy,
};
use crate::prompting::{
    format_initial_prompt_with_metadata, format_interprocess_user_message_with_metadata,
    format_system_injection_with_metadata, format_user_message_with_metadata, GenerationConfig,
};

use super::tokenizer::{
    build_remote_fallback_tokenizer, resolve_special_tokens, resolve_tokenizer_path,
};
use super::LLMEngine;

impl LLMEngine {
    pub fn load_target(target: &ResolvedModelTarget) -> Result<Self> {
        Self::load(
            target.display_path().to_string_lossy().as_ref(),
            target.runtime_reference(),
            target.family(),
            target.cloned_tokenizer_path(),
            target.cloned_metadata(),
            target.driver_resolution().clone(),
            target,
        )
    }

    fn load(
        display_path: &str,
        runtime_reference: &str,
        family: crate::prompting::PromptFamily,
        tokenizer_hint: Option<PathBuf>,
        metadata: Option<ModelMetadata>,
        driver_resolution: DriverResolution,
        target: &ResolvedModelTarget,
    ) -> Result<Self> {
        tracing::info!(
            display_path,
            runtime_reference,
            ?family,
            "ENGINE: Loading Master Model"
        );

        let model = RuntimeModel::load_target(target)?;
        let resolved_family = model.family();
        let backend_id = model.backend_id();

        tracing::info!(
            backend_id,
            resolution_source = driver_resolution.resolution_source,
            resolution_rationale = driver_resolution.resolution_rationale,
            ?resolved_family,
            "ENGINE: Weights loaded. Loading Tokenizer..."
        );

        let tokenizer = if let Some(tokenizer_path) =
            resolve_tokenizer_path(display_path, tokenizer_hint)
        {
            tracing::info!(?tokenizer_path, "ENGINE: Using tokenizer");
            tokenizers::Tokenizer::from_file(tokenizer_path).map_err(E::msg)?
        } else if driver_resolution.backend_class == crate::backend::BackendClass::RemoteStateless {
            tracing::warn!(
                backend_id,
                "ENGINE: Remote stateless backend loaded without tokenizer hint; using fallback whitespace tokenizer"
            );
            build_remote_fallback_tokenizer()
        } else {
            return Err(E::msg(
                "Tokenizer not found for selected model (fail-fast policy).",
            ));
        };

        let (eos_token_id, eot_token_id) =
            resolve_special_tokens(&tokenizer, resolved_family, metadata.as_ref())
                .map_err(E::msg)?;

        tracing::info!(
            eos_token_id,
            eot_token_id,
            "ENGINE: Special Tokens Identified"
        );
        tracing::info!("ENGINE: Master Model & Tokenizer Ready. Backend abstraction enabled.");

        Ok(Self {
            master_model: Some(model),
            display_path: display_path.to_string(),
            runtime_reference: runtime_reference.to_string(),
            backend_id: backend_id.to_string(),
            driver_resolution_source: driver_resolution.resolution_source.to_string(),
            driver_resolution_rationale: driver_resolution.resolution_rationale,
            loaded_remote_model: target.remote_model_view(),
            tokenizer,
            processes: std::collections::HashMap::new(),
            next_pid: 1,
            family: resolved_family,
            metadata,
            generation: crate::policy::generation_defaults(resolved_family),
            eos_token_id,
            eot_token_id,
            resident_slot_manager: super::slot_manager::ResidentSlotManager::new(),
        })
    }

    pub fn spawn_process(
        &mut self,
        prompt: &str,
        system_prompt: Option<&str>,
        _max_tokens: usize,
        owner_id: usize,
        lifecycle_policy: ProcessLifecyclePolicy,
        context_policy: ContextPolicy,
    ) -> Result<u64> {
        let pid = self.next_pid;
        self.next_pid += 1;

        tracing::info!(pid, owner_id, "OS: Forking Agent Process (Zero-Copy)");

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
                RuntimeModel::load_from_reference(
                    &self.runtime_reference,
                    self.family,
                    &self.backend_id,
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

        let formatted_prompt = format_initial_prompt_with_metadata(
            system_prompt,
            prompt,
            self.family,
            self.metadata.as_ref(),
        );

        let tokens = self
            .tokenizer
            .encode(formatted_prompt.as_str(), true)
            .map_err(E::msg)?
            .get_ids()
            .to_vec();

        let process = AgentProcess::new(
            pid,
            owner_id,
            lifecycle_policy,
            model_clone,
            self.tokenizer.clone(),
            tokens,
            self.generation,
            InitialContextSeed {
                policy: context_policy,
                initial_segment_text: formatted_prompt,
            },
        );
        self.processes.insert(pid, process);

        Ok(pid)
    }

    pub fn ensure_next_pid_at_least(&mut self, next_pid: u64) {
        if self.next_pid < next_pid {
            self.next_pid = next_pid;
        }
    }

    pub fn set_process_context_slot(&mut self, pid: u64, slot_id: ContextSlotId) -> Result<()> {
        let policy = self
            .processes
            .get(&pid)
            .map(|process| {
                super::slot_manager::ResidentSlotManager::policy_for_capabilities(
                    process.model.backend_capabilities(),
                )
            })
            .unwrap_or(ResidentSlotPolicy::Unmanaged);
        let process = self
            .processes
            .get_mut(&pid)
            .ok_or(E::msg("PID not found"))?;
        process.bind_context_slot(slot_id, policy);
        self.resident_slot_manager.bind(pid, slot_id, policy);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn save_process_context_slot(&mut self, pid: u64, path: &std::path::Path) -> Result<()> {
        let slot_id = self
            .processes
            .get(&pid)
            .ok_or_else(|| E::msg("PID not found"))?
            .context_slot_id
            .ok_or_else(|| E::msg(format!("PID {} has no assigned context slot", pid)))?;

        let model = self
            .master_model
            .as_ref()
            .ok_or_else(|| E::msg("Master model not loaded"))?;
        model.save_context_slot(slot_id, path)?;

        let process = self
            .processes
            .get_mut(&pid)
            .ok_or_else(|| E::msg("PID not found"))?;
        process.mark_resident_slot_snapshot_saved(path.to_path_buf());
        Ok(())
    }

    pub fn mark_process_context_slot_saved(
        &mut self,
        pid: u64,
        path: &std::path::Path,
    ) -> Result<()> {
        let process = self
            .processes
            .get_mut(&pid)
            .ok_or_else(|| E::msg("PID not found"))?;
        if process.context_slot_id.is_none() {
            return Err(E::msg(format!("PID {} has no assigned context slot", pid)));
        }
        self.resident_slot_manager.mark_snapshot_saved(pid, path);
        process.mark_resident_slot_snapshot_saved(path.to_path_buf());
        Ok(())
    }

    pub fn load_process_context_slot(&mut self, pid: u64, path: &std::path::Path) -> Result<()> {
        let slot_id = self
            .processes
            .get(&pid)
            .ok_or_else(|| E::msg("PID not found"))?
            .context_slot_id
            .ok_or_else(|| E::msg(format!("PID {} has no assigned context slot", pid)))?;

        if let Some(process) = self.processes.get_mut(&pid) {
            self.resident_slot_manager.mark_restoring(pid, path);
            process.mark_resident_slot_restoring(path.to_path_buf());
        }

        let model = self
            .master_model
            .as_mut()
            .ok_or_else(|| E::msg("Master model not loaded"))?;
        if let Err(err) = model.load_context_slot(slot_id, path) {
            if let Some(process) = self.processes.get_mut(&pid) {
                process.mark_resident_slot_snapshot_saved(path.to_path_buf());
            }
            return Err(err);
        }

        if let Some(process) = self.processes.get_mut(&pid) {
            self.resident_slot_manager.mark_allocated(pid);
            process.mark_resident_slot_allocated();
            process.mark_resident_prompt_checkpoint();
        }
        Ok(())
    }

    pub fn free_process_context_slot(&mut self, pid: u64) -> Result<()> {
        let slot_id = self
            .processes
            .get(&pid)
            .ok_or_else(|| E::msg("PID not found"))?
            .context_slot_id
            .ok_or_else(|| E::msg(format!("PID {} has no assigned context slot", pid)))?;

        let model = self
            .master_model
            .as_mut()
            .ok_or_else(|| E::msg("Master model not loaded"))?;
        model.free_context_slot(slot_id)?;

        if let Some(process) = self.processes.get_mut(&pid) {
            self.resident_slot_manager.mark_allocated(pid);
            process.mark_resident_slot_allocated();
            process.reset_resident_prompt_checkpoint();
        }
        Ok(())
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
        let injected_len = new_tokens.len();

        tracing::debug!(pid, tokens = new_tokens.len(), "OS: Injecting tokens");
        process.tokens.extend(new_tokens);
        process.record_injected_context(&formatted_text, injected_len);
        process.state = ProcessState::Running;
        Ok(())
    }

    pub fn send_user_input(&mut self, pid: u64, prompt: &str) -> Result<()> {
        let formatted_prompt =
            format_user_message_with_metadata(prompt, self.family, self.metadata.as_ref());

        let process = self
            .processes
            .get_mut(&pid)
            .ok_or(E::msg("PID not found"))?;

        if process.state != ProcessState::WaitingForInput {
            return Err(E::msg(format!(
                "PID {} is not waiting for input (state={:?})",
                pid, process.state
            )));
        }

        let new_tokens = process
            .tokenizer
            .encode(formatted_prompt.as_str(), true)
            .map_err(E::msg)?
            .get_ids()
            .to_vec();
        let token_count = new_tokens.len();

        process.tokens.extend(new_tokens);
        process.record_user_input(&formatted_prompt, token_count);
        process.begin_next_turn();
        process.state = ProcessState::Ready;
        Ok(())
    }

    pub fn continue_current_turn(&mut self, pid: u64) -> Result<()> {
        let process = self
            .processes
            .get_mut(&pid)
            .ok_or(E::msg("PID not found"))?;

        if process.state != ProcessState::AwaitingTurnDecision {
            return Err(E::msg(format!(
                "PID {} is not awaiting a turn decision (state={:?})",
                pid, process.state
            )));
        }

        process.extend_current_turn_budget();
        process.state = ProcessState::Ready;
        Ok(())
    }

    pub fn stop_current_turn(&mut self, pid: u64) -> Result<()> {
        let process = self
            .processes
            .get_mut(&pid)
            .ok_or(E::msg("PID not found"))?;

        if process.state != ProcessState::AwaitingTurnDecision {
            return Err(E::msg(format!(
                "PID {} is not awaiting a turn decision (state={:?})",
                pid, process.state
            )));
        }

        process.abandon_current_turn();
        process.state = ProcessState::WaitingForInput;
        Ok(())
    }

    pub fn list_active_pids(&self) -> Vec<u64> {
        self.processes
            .iter()
            .filter_map(|(pid, proc)| {
                if matches!(proc.state, ProcessState::Ready | ProcessState::Running) {
                    Some(*pid)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn list_parked_pids(&self) -> Vec<u64> {
        self.processes
            .iter()
            .filter_map(|(pid, proc)| {
                if proc.state == ProcessState::Parked {
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
        self.resident_slot_manager.remove(pid);
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

    pub fn park_process(&mut self, pid: u64) -> bool {
        if let Some(proc) = self.processes.get_mut(&pid) {
            if proc.state != ProcessState::Finished {
                if matches!(
                    self.resident_slot_manager.mark_park_requested(pid),
                    Some(ResidentSlotPolicy::ParkAndResume)
                ) {
                    proc.mark_resident_slot_park_requested();
                }
                proc.state = ProcessState::Parked;
                return true;
            }
        }
        false
    }

    pub fn set_process_ready_if_parked(&mut self, pid: u64) -> bool {
        if let Some(proc) = self.processes.get_mut(&pid) {
            if proc.state == ProcessState::Parked {
                proc.state = ProcessState::Ready;
                return true;
            }
        }
        false
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
        &self.display_path
    }

    pub fn loaded_backend_id(&self) -> &str {
        &self.backend_id
    }

    pub fn loaded_backend_class(&self) -> crate::backend::BackendClass {
        self.master_model
            .as_ref()
            .map(|model| model.backend_class())
            .unwrap_or(crate::backend::BackendClass::RemoteStateless)
    }

    pub fn loaded_backend_capabilities(&self) -> crate::backend::BackendCapabilities {
        self.master_model
            .as_ref()
            .map(|model| model.backend_capabilities())
            .unwrap_or_default()
    }

    pub fn driver_resolution_source(&self) -> &str {
        &self.driver_resolution_source
    }

    pub fn driver_resolution_rationale(&self) -> &str {
        &self.driver_resolution_rationale
    }

    pub fn loaded_remote_model(&self) -> Option<&agentic_control_models::RemoteModelRuntimeView> {
        self.loaded_remote_model.as_ref()
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

    #[allow(dead_code)]
    pub fn load_context_slot(
        &mut self,
        slot_id: ContextSlotId,
        path: &std::path::Path,
    ) -> Result<()> {
        let model = self
            .master_model
            .as_mut()
            .ok_or_else(|| E::msg("Master model not loaded"))?;
        model.load_context_slot(slot_id, path)
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
#[path = "lifecycle_tests.rs"]
mod tests;
