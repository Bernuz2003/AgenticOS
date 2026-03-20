use super::LLMEngine;
use crate::backend::{
    ContextSlotPersistence, InferenceBackend, InferenceStepRequest, InferenceStepResult,
    RuntimeModel,
};
use crate::engine::slot_manager::ResidentSlotManager;
use crate::memory::ContextSlotId;
use crate::model_catalog::ModelCatalog;
use crate::process::{
    AgentProcess, ContextPolicy, ContextStrategy, HumanInputRequest, HumanInputRequestKind,
    InitialContextSeed, ProcessLifecyclePolicy, ProcessState, ResidentSlotPolicy,
    ResidentSlotState,
};
use crate::prompting::GenerationConfig;
use crate::prompting::PromptFamily;
use crate::tools::invocation::{ProcessPermissionPolicy, ProcessTrustScope, ToolCaller};
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

    fn generate_step(&mut self, _request: InferenceStepRequest<'_>) -> Result<InferenceStepResult> {
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

type SavedSlots = Arc<Mutex<Vec<(ContextSlotId, String)>>>;
type FreedSlots = Arc<Mutex<Vec<ContextSlotId>>>;

fn test_engine() -> (LLMEngine, SavedSlots, SavedSlots, FreedSlots) {
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
        ToolCaller::AgentText,
        ProcessPermissionPolicy {
            trust_scope: ProcessTrustScope::InteractiveChat,
            actions_allowed: false,
            allowed_tools: Vec::new(),
            path_scopes: vec![".".to_string()],
        },
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
            display_path: "test.gguf".to_string(),
            runtime_reference: "test.gguf".to_string(),
            backend_id: "external-llamacpp".to_string(),
            driver_resolution_source: "test".to_string(),
            driver_resolution_rationale: "test".to_string(),
            loaded_remote_model: None,
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
    let catalog =
        ModelCatalog::discover(crate::config::repository_path("models")).expect("discover models");
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

#[test]
fn send_user_input_clears_pending_human_request_and_resumes_process() {
    let (mut engine, _saves, _loads, _frees) = test_engine();
    let process = engine.processes.get_mut(&1).expect("process present");
    process.state = ProcessState::WaitingForInput;
    process.set_pending_human_request(HumanInputRequest {
        kind: HumanInputRequestKind::Approval,
        question: "Ship this workflow?".to_string(),
        details: None,
        choices: vec!["approve".to_string(), "reject".to_string()],
        allow_free_text: false,
        placeholder: None,
        requested_at_ms: 1234,
    });

    engine
        .send_user_input(1, "approve")
        .expect("human reply accepted");

    let process = engine.processes.get(&1).expect("process present");
    assert_eq!(process.state, ProcessState::Ready);
    assert!(process.pending_human_request.is_none());
}
