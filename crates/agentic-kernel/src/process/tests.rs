/// Unit tests for process and context management.
use super::{
    AgentProcess, ContextPolicy, ContextSegment, ContextSegmentKind, ContextStrategy,
    InitialContextSeed, ProcessLifecyclePolicy, ResidentSlotPolicy,
};
use crate::backend::{
    ContextSlotPersistence, InferenceBackend, InferenceStepRequest, InferenceStepResult,
    RuntimeModel,
};
use crate::memory::ContextSlotId;
use crate::prompting::GenerationConfig;
use crate::tools::invocation::{ProcessPermissionPolicy, ProcessTrustScope, ToolCaller};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokenizers::models::wordlevel::WordLevel;
use tokenizers::pre_tokenizers::whitespace::Whitespace;
use tokenizers::Tokenizer;

#[derive(Clone)]
struct RecordingBackend {
    frees: Arc<Mutex<Vec<ContextSlotId>>>,
}

impl InferenceBackend for RecordingBackend {
    fn backend_id(&self) -> &'static str {
        "recording"
    }

    fn family(&self) -> crate::prompting::PromptFamily {
        crate::prompting::PromptFamily::Unknown
    }

    fn generate_step(&mut self, _request: InferenceStepRequest<'_>) -> Result<InferenceStepResult> {
        Ok(InferenceStepResult {
            appended_tokens: Vec::new(),
            emitted_text: String::new(),
            finished: true,
            finish_reason: None,
            next_index_pos: 0,
        })
    }

    fn duplicate_boxed(&self) -> Option<Box<dyn crate::backend::ModelBackend>> {
        Some(Box::new(self.clone()))
    }
}

impl ContextSlotPersistence for RecordingBackend {
    fn save_context_slot(&self, _slot_id: ContextSlotId, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn load_context_slot(&mut self, _slot_id: ContextSlotId, _path: &Path) -> Result<()> {
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
        ("assistant".to_string(), 3),
        ("reply".to_string(), 4),
        ("system".to_string(), 5),
        ("note".to_string(), 6),
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

fn test_process(policy: ContextPolicy) -> (AgentProcess, Arc<Mutex<Vec<ContextSlotId>>>) {
    let frees = Arc::new(Mutex::new(Vec::new()));
    let model = RuntimeModel::from_boxed_backend(Box::new(RecordingBackend {
        frees: Arc::clone(&frees),
    }));
    let tokenizer = test_tokenizer();
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
        model,
        tokenizer,
        vec![1, 2, 1],
        GenerationConfig {
            temperature: 0.7,
            top_p: 0.9,
            seed: 1,
            max_tokens: 64,
        },
        InitialContextSeed {
            policy,
            initial_segment_text: "user turn user".to_string(),
        },
    );
    (process, frees)
}

fn append_segment_tokens(
    process: &mut AgentProcess,
    kind: ContextSegmentKind,
    text: &str,
    tokens: &[u32],
) {
    process.tokens.extend(tokens.iter().copied());
    process
        .context_state
        .segments
        .push(ContextSegment::new(kind, tokens.len(), text.to_string()));
    process.context_state.tokens_used = process.tokens.len();
    process.rebuild_rendered_prompt_cache();
}

#[test]
fn sliding_window_drops_complete_segments_and_resets_backend_slot() {
    let policy = ContextPolicy::new(ContextStrategy::SlidingWindow, 8, 7, 5, 3);
    let (mut process, frees) = test_process(policy);
    process.bind_context_slot(11, ResidentSlotPolicy::ParkAndResume);
    process.tokens.extend([5, 6, 3, 4, 4, 4]);
    process.record_injected_context("system note", 2);
    process.record_model_output("assistant reply reply reply", 4);
    process.index_pos = process.tokens.len();

    let event = process
        .enforce_context_budget()
        .expect("compaction should run");

    assert_eq!(event.dropped_segments, 2);
    assert_eq!(event.dropped_tokens, 5);
    assert_eq!(process.tokens.len(), 4);
    assert_eq!(process.index_pos, 0);
    assert_eq!(process.context_state.context_compressions, 1);
    assert_eq!(process.context_state.segments.len(), 1);
    assert_eq!(
        process.context_state.segments[0].kind,
        ContextSegmentKind::AssistantTurn
    );
    assert_eq!(process.prompt_text(), "assistant reply reply reply");
    assert_eq!(*frees.lock().expect("lock frees"), vec![11]);
}

#[test]
fn rendered_prompt_cache_tracks_append_only_turn_updates() {
    let policy = ContextPolicy::new(ContextStrategy::SlidingWindow, 32, 24, 16, 2);
    let (mut process, _frees) = test_process(policy);

    assert_eq!(process.prompt_text(), "user turn user");

    process.record_injected_context("\nsystem note\n", 2);
    process.record_model_output("assistant reply", 2);
    process.record_user_input("\nuser followup\n", 2);

    assert_eq!(
        process.prompt_text(),
        "user turn user\nsystem note\nassistant reply\nuser followup\n"
    );
}

#[test]
fn resident_prompt_suffix_only_tracks_post_checkpoint_reinjection() {
    let policy = ContextPolicy::new(ContextStrategy::SlidingWindow, 32, 24, 16, 2);
    let (mut process, _frees) = test_process(policy);

    process.bind_context_slot(9, ResidentSlotPolicy::ParkAndResume);
    process.record_model_output("assistant reply", 2);
    process.mark_resident_prompt_checkpoint();

    assert_eq!(process.pending_resident_prompt_suffix(), "");

    process.record_injected_context("\nOutput:\n2\n", 3);

    assert_eq!(process.pending_resident_prompt_suffix(), "\nOutput:\n2\n");
}

#[test]
fn sliding_window_reports_overflow_when_only_one_segment_remains() {
    let policy = ContextPolicy::new(ContextStrategy::SlidingWindow, 4, 4, 3, 3);
    let (mut process, _frees) = test_process(policy);
    process.tokens.extend([1, 2, 1]);
    process.context_state.segments[0].token_count = process.tokens.len();
    process.context_state.segments[0].text = "single giant turn".to_string();

    let event = process.enforce_context_budget();

    assert!(event.is_none());
    assert_eq!(process.tokens.len(), 6);
    assert_eq!(
        process.context_state.last_compaction_reason.as_deref(),
        Some("sliding_window_no_complete_segment_fit")
    );
}

#[test]
fn summarize_replaces_old_segments_with_summary_segment() {
    let policy = ContextPolicy::new(ContextStrategy::Summarize, 8, 7, 5, 3);
    let (mut process, frees) = test_process(policy);
    process.bind_context_slot(9, ResidentSlotPolicy::ParkAndResume);
    process.tokens.extend([5, 6, 3, 4, 4, 4]);
    process.record_injected_context("system note", 2);
    process.record_model_output("assistant reply reply reply", 4);
    process.index_pos = process.tokens.len();

    let event = process
        .enforce_context_budget()
        .expect("summary compaction should run");

    assert_eq!(event.strategy, ContextStrategy::Summarize);
    assert_eq!(
        process.context_state.segments[0].kind,
        ContextSegmentKind::Summary
    );
    assert!(process.context_state.last_summary_ts.is_some());
    assert!(process.tokens.len() <= process.context_policy.compaction_target_tokens);
    assert_eq!(*frees.lock().expect("lock frees"), vec![9]);
}

#[test]
fn retrieve_archives_old_segments_and_reinjects_top_k_context() {
    let policy = ContextPolicy::new(ContextStrategy::Retrieve, 8, 7, 5, 2);
    let (mut process, frees) = test_process(policy);
    process.bind_context_slot(13, ResidentSlotPolicy::ParkAndResume);
    process.tokens.extend([5, 6, 3, 4, 4, 4]);
    process.record_injected_context("system note", 2);
    process.record_model_output("assistant reply reply reply", 4);
    process.index_pos = process.tokens.len();

    let event = process
        .enforce_context_budget()
        .expect("retrieve compaction should run");

    assert_eq!(event.strategy, ContextStrategy::Retrieve);
    assert_eq!(process.context_state.episodic_segments.len(), 2);
    assert_eq!(
        process.context_state.segments[0].kind,
        ContextSegmentKind::RetrievedMemory
    );
    assert_eq!(
        process.context_state.segments[1].kind,
        ContextSegmentKind::AssistantTurn
    );
    assert_eq!(process.context_state.context_retrieval_hits, 1);
    assert!(process.tokens.len() <= process.context_policy.window_size_tokens);
    assert_eq!(*frees.lock().expect("lock frees"), vec![13]);
}

#[test]
fn retrieve_prefers_overlap_before_pure_recency() {
    let policy = ContextPolicy::new(ContextStrategy::Retrieve, 24, 12, 10, 2);
    let (mut process, _frees) = test_process(policy);
    process.context_state.episodic_segments = vec![
        ContextSegment::new(
            ContextSegmentKind::AssistantTurn,
            2,
            "generic archive".to_string(),
        ),
        ContextSegment::new(
            ContextSegmentKind::AssistantTurn,
            2,
            "kernel scheduler quota".to_string(),
        ),
    ];
    process.context_state.segments = vec![ContextSegment::new(
        ContextSegmentKind::UserTurn,
        3,
        "explain scheduler quota".to_string(),
    )];
    process.tokens = vec![1, 2, 1];
    process.context_state.tokens_used = process.tokens.len();

    let payload = process
        .build_retrieval_payload(&process.context_state.episodic_segments, 8)
        .expect("retrieval payload");

    assert!(payload.0.contains("kernel scheduler quota"));
}

#[test]
fn long_running_multi_turn_strategies_remain_bounded_and_observable() {
    let strategies = [
        ContextStrategy::SlidingWindow,
        ContextStrategy::Summarize,
        ContextStrategy::Retrieve,
    ];

    for (offset, strategy) in strategies.into_iter().enumerate() {
        let policy = ContextPolicy::new(strategy, 12, 10, 8, 2);
        let (mut process, frees) = test_process(policy);
        process.bind_context_slot((offset as u64) + 21, ResidentSlotPolicy::ParkAndResume);

        for turn in 0..8 {
            append_segment_tokens(
                &mut process,
                ContextSegmentKind::UserTurn,
                &format!("user turn {} scheduler quota", turn),
                &[1, 2],
            );
            append_segment_tokens(
                &mut process,
                ContextSegmentKind::AssistantTurn,
                &format!("assistant reply {} scheduler", turn),
                &[3, 4],
            );

            let _ = process.enforce_context_budget();

            assert!(
                process.tokens.len() <= process.context_policy.window_size_tokens,
                "strategy {} exceeded window",
                strategy.label()
            );
            assert!(!process.context_state.segments.is_empty());
            assert!(process.context_state.tokens_used <= process.context_policy.window_size_tokens);
        }

        match strategy {
            ContextStrategy::SlidingWindow => {
                assert!(process.context_state.context_compressions > 0);
                assert!(process.context_state.last_compaction_reason.is_some());
            }
            ContextStrategy::Summarize => {
                assert!(process.context_state.context_compressions > 0);
                assert!(process.context_state.last_summary_ts.is_some());
                assert!(process
                    .context_state
                    .segments
                    .iter()
                    .any(|segment| { segment.kind == ContextSegmentKind::Summary }));
            }
            ContextStrategy::Retrieve => {
                assert!(process.context_state.last_compaction_reason.is_some());
                assert!(process.context_state.context_retrieval_hits > 0);
                assert!(!process.context_state.episodic_segments.is_empty());
            }
        }
        assert!(!frees.lock().expect("lock frees").is_empty());
    }
}

#[test]
fn resident_slot_state_tracks_snapshot_lifecycle() {
    let policy = ContextPolicy::new(ContextStrategy::SlidingWindow, 8, 7, 5, 3);
    let (mut process, _frees) = test_process(policy);

    assert_eq!(
        process.resident_slot_state,
        super::ResidentSlotState::Unbound
    );
    assert_eq!(process.resident_slot_state_label(), None);

    process.bind_context_slot(17, ResidentSlotPolicy::ParkAndResume);
    assert_eq!(
        process.resident_slot_state,
        super::ResidentSlotState::Allocated
    );
    assert_eq!(
        process.resident_slot_state_label().as_deref(),
        Some("allocated")
    );

    process.mark_resident_slot_snapshot_saved(PathBuf::from("workspace/swap/pid_17_slot_17.swap"));
    assert_eq!(
        process.resident_slot_state,
        super::ResidentSlotState::SnapshotSaved
    );
    assert_eq!(
        process.resident_slot_snapshot_path(),
        Some(Path::new("workspace/swap/pid_17_slot_17.swap"))
    );

    process.mark_resident_slot_restoring(PathBuf::from("workspace/swap/pid_17_slot_17.swap"));
    assert_eq!(
        process.resident_slot_state,
        super::ResidentSlotState::Restoring
    );

    process.mark_resident_slot_allocated();
    assert_eq!(
        process.resident_slot_state,
        super::ResidentSlotState::Allocated
    );
}
