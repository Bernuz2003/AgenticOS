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
    format_interprocess_user_message_with_metadata, format_system_injection_with_metadata,
    format_user_message_with_metadata, GenerationConfig,
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

        let model =
            RuntimeModel::load_from_gguf(path, family, &driver_resolution.resolved_backend_id)?;
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
            model_path: path.to_string(),
            backend_id: backend_id.to_string(),
            driver_resolution_source: driver_resolution.resolution_source.to_string(),
            driver_resolution_rationale: driver_resolution.resolution_rationale,
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
                RuntimeModel::load_from_gguf(&self.model_path, self.family, &self.backend_id)?
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

        let formatted_prompt =
            format_user_message_with_metadata(prompt, self.family, self.metadata.as_ref());

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
mod tests {
    use super::LLMEngine;
    use crate::backend::{
        ContextSlotPersistence, InferenceBackend, InferenceStepRequest, InferenceStepResult,
        RuntimeModel,
    };
    use crate::engine::slot_manager::ResidentSlotManager;
    use crate::memory::ContextSlotId;
    use crate::model_catalog::ModelCatalog;
    use crate::process::{
        AgentProcess, ContextPolicy, ContextStrategy, InitialContextSeed, ProcessLifecyclePolicy,
        ResidentSlotPolicy, ResidentSlotState,
    };
    use crate::prompting::GenerationConfig;
    use crate::prompting::PromptFamily;
    use anyhow::Result;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tokenizers::models::wordlevel::WordLevel;
    use tokenizers::pre_tokenizers::whitespace::Whitespace;
    use tokenizers::Tokenizer;

    #[derive(Clone)]
    struct RecordingBackend {
        saves: Arc<Mutex<Vec<(ContextSlotId, String)>>>,
        loads: Arc<Mutex<Vec<(ContextSlotId, String)>>>,
        frees: Arc<Mutex<Vec<ContextSlotId>>>,
    }

    impl InferenceBackend for RecordingBackend {
        fn backend_id(&self) -> &'static str {
            "external-llamacpp"
        }

        fn family(&self) -> PromptFamily {
            PromptFamily::Qwen
        }

        fn generate_step(
            &mut self,
            _request: InferenceStepRequest<'_>,
        ) -> Result<InferenceStepResult> {
            panic!("generate_step should not be called in lifecycle tests");
        }

        fn duplicate_boxed(&self) -> Option<Box<dyn crate::backend::ModelBackend>> {
            Some(Box::new(self.clone()))
        }
    }

    impl ContextSlotPersistence for RecordingBackend {
        fn save_context_slot(&self, slot_id: ContextSlotId, path: &Path) -> Result<()> {
            self.saves
                .lock()
                .expect("lock saves")
                .push((slot_id, path.display().to_string()));
            Ok(())
        }

        fn load_context_slot(&mut self, slot_id: ContextSlotId, path: &Path) -> Result<()> {
            self.loads
                .lock()
                .expect("lock loads")
                .push((slot_id, path.display().to_string()));
            Ok(())
        }

        fn free_context_slot(&mut self, slot_id: ContextSlotId) -> Result<()> {
            self.frees.lock().expect("lock frees").push(slot_id);
            Ok(())
        }
    }

    fn test_tokenizer() -> Tokenizer {
        let vocab = [
            ("<unk>".to_string(), 0),
            ("user".to_string(), 1),
            ("turn".to_string(), 2),
        ]
        .into_iter()
        .collect();
        let model = WordLevel::builder()
            .vocab(vocab)
            .unk_token("<unk>".to_string())
            .build()
            .expect("build wordlevel");
        let mut tokenizer = Tokenizer::new(model);
        tokenizer.with_pre_tokenizer(Some(Whitespace));
        tokenizer
    }

    fn test_engine() -> (
        LLMEngine,
        Arc<Mutex<Vec<(ContextSlotId, String)>>>,
        Arc<Mutex<Vec<(ContextSlotId, String)>>>,
        Arc<Mutex<Vec<ContextSlotId>>>,
    ) {
        let saves = Arc::new(Mutex::new(Vec::new()));
        let loads = Arc::new(Mutex::new(Vec::new()));
        let frees = Arc::new(Mutex::new(Vec::new()));
        let master_model = RuntimeModel::from_boxed_backend(Box::new(RecordingBackend {
            saves: Arc::clone(&saves),
            loads: Arc::clone(&loads),
            frees: Arc::clone(&frees),
        }));
        let process_model = master_model
            .duplicate_if_supported()
            .expect("recording backend should duplicate");
        let tokenizer = test_tokenizer();
        let generation = GenerationConfig {
            temperature: 0.7,
            top_p: 0.9,
            seed: 1,
            max_tokens: 64,
        };
        let process = AgentProcess::new(
            1,
            7,
            ProcessLifecyclePolicy::Interactive,
            process_model,
            tokenizer.clone(),
            vec![1, 2],
            generation,
            InitialContextSeed {
                policy: ContextPolicy::new(ContextStrategy::SlidingWindow, 64, 64, 32, 4),
                initial_segment_text: "user turn".to_string(),
            },
        );

        let mut processes = HashMap::new();
        processes.insert(1, process);

        (
            LLMEngine {
                master_model: Some(master_model),
                model_path: "test.gguf".to_string(),
                backend_id: "external-llamacpp".to_string(),
                driver_resolution_source: "test".to_string(),
                driver_resolution_rationale: "test".to_string(),
                tokenizer,
                processes,
                next_pid: 2,
                family: PromptFamily::Qwen,
                metadata: None,
                generation,
                eos_token_id: 0,
                eot_token_id: 0,
                resident_slot_manager: ResidentSlotManager::new(),
            },
            saves,
            loads,
            frees,
        )
    }

    #[test]
    #[ignore = "uses the real local Qwen3.5 artifacts to validate generic discovery and rejection"]
    fn qwen35_catalog_target_is_rejected_before_backend_load() {
        let model_id = "qwen3.5-9b/Qwen3.5-9B-Q4_K_M";
        let catalog = ModelCatalog::discover(crate::config::repository_path("models"))
            .expect("discover models");
        let entry = catalog.find_by_id(model_id).expect("qwen3.5 entry present");

        assert_eq!(entry.family, PromptFamily::Qwen);
        assert!(
            entry.tokenizer_path.is_some(),
            "qwen3.5 tokenizer must be discoverable"
        );
        assert_eq!(
            entry
                .metadata
                .as_ref()
                .and_then(|meta| meta.architecture.as_deref()),
            Some("qwen35")
        );

        let err = catalog.resolve_load_target(model_id).expect_err(
            "qwen3.5 should fail generic driver resolution until a compatible backend exists",
        );
        assert!(err.to_string().contains("qwen35"));
    }

    #[test]
    fn pid_based_context_slot_lifecycle_updates_process_metadata() {
        let (mut engine, saves, loads, frees) = test_engine();
        let snapshot_path = Path::new("workspace/swap/pid_1_slot_7.swap");

        engine
            .set_process_context_slot(1, 7)
            .expect("assign process slot");
        assert_eq!(
            engine
                .processes
                .get(&1)
                .expect("process present")
                .resident_slot_state,
            ResidentSlotState::Allocated
        );

        engine
            .save_process_context_slot(1, snapshot_path)
            .expect("save process slot");
        assert_eq!(
            engine
                .processes
                .get(&1)
                .expect("process present")
                .resident_slot_state,
            ResidentSlotState::SnapshotSaved
        );

        engine
            .load_process_context_slot(1, snapshot_path)
            .expect("load process slot");
        assert_eq!(
            engine
                .processes
                .get(&1)
                .expect("process present")
                .resident_slot_state,
            ResidentSlotState::Allocated
        );
        assert_eq!(
            engine
                .processes
                .get(&1)
                .expect("process present")
                .resident_slot_snapshot_path(),
            Some(snapshot_path)
        );
        assert_eq!(
            engine
                .processes
                .get(&1)
                .expect("process present")
                .pending_resident_prompt_suffix(),
            ""
        );

        engine
            .free_process_context_slot(1)
            .expect("free process slot");
        assert_eq!(
            engine
                .processes
                .get(&1)
                .expect("process present")
                .resident_slot_state,
            ResidentSlotState::Allocated
        );
        assert_eq!(
            engine
                .processes
                .get(&1)
                .expect("process present")
                .pending_resident_prompt_suffix(),
            engine
                .processes
                .get(&1)
                .expect("process present")
                .prompt_text()
        );

        assert_eq!(
            saves.lock().expect("lock saves").as_slice(),
            &[(7, snapshot_path.display().to_string())]
        );
        assert_eq!(
            loads.lock().expect("lock loads").as_slice(),
            &[(7, snapshot_path.display().to_string())]
        );
        assert_eq!(frees.lock().expect("lock frees").as_slice(), &[7]);
    }

    #[test]
    fn mark_process_context_slot_saved_requires_bound_slot() {
        let (mut engine, _saves, _loads, _frees) = test_engine();
        let err = engine
            .mark_process_context_slot_saved(1, Path::new("workspace/swap/pid_1_slot_7.swap"))
            .expect_err("unbound process should reject resident slot snapshot bookkeeping");
        assert!(err.to_string().contains("no assigned context slot"));
    }

    #[test]
    fn park_process_marks_resident_slot_policy_and_state_explicitly() {
        let (mut engine, _saves, _loads, _frees) = test_engine();

        engine
            .set_process_context_slot(1, 7)
            .expect("assign process slot");
        assert!(engine.park_process(1));

        let process = engine.processes.get(&1).expect("process present");
        assert_eq!(
            process.resident_slot_policy,
            ResidentSlotPolicy::ParkAndResume
        );
        assert_eq!(
            process.resident_slot_state,
            ResidentSlotState::ParkRequested
        );
        assert_eq!(
            engine.resident_slot_manager.lease_for(1),
            Some((
                7,
                ResidentSlotPolicy::ParkAndResume,
                ResidentSlotState::ParkRequested
            ))
        );
    }
}
