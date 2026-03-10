use candle_transformers::generation::LogitsProcessor;
use serde::{Deserialize, Serialize};
use tokenizers::Tokenizer;

use crate::backend::RuntimeModel;
use crate::memory::ContextSlotId;
use crate::prompting::GenerationConfig;

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessState {
    Ready,
    Running,
    WaitingForMemory,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContextStrategy {
    #[default]
    SlidingWindow,
    Summarize,
    Retrieve,
}

impl ContextStrategy {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "sliding" | "sliding_window" => Some(Self::SlidingWindow),
            "summarize" | "summary" => Some(Self::Summarize),
            "retrieve" | "retrieval" => Some(Self::Retrieve),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SlidingWindow => "sliding_window",
            Self::Summarize => "summarize",
            Self::Retrieve => "retrieve",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPolicy {
    pub strategy: ContextStrategy,
    pub window_size_tokens: usize,
    pub compaction_trigger_tokens: usize,
    pub compaction_target_tokens: usize,
    pub retrieve_top_k: usize,
}

impl ContextPolicy {
    pub fn from_kernel_defaults() -> Self {
        let config = &crate::config::kernel_config().context;
        let strategy = ContextStrategy::parse(&config.default_strategy).unwrap_or_default();
        Self::new(
            strategy,
            config.default_window_tokens,
            config.compaction_trigger_tokens,
            config.compaction_target_tokens,
            config.retrieve_top_k,
        )
    }

    pub fn new(
        strategy: ContextStrategy,
        window_size_tokens: usize,
        compaction_trigger_tokens: usize,
        compaction_target_tokens: usize,
        retrieve_top_k: usize,
    ) -> Self {
        let window_size_tokens = window_size_tokens.max(1);
        let compaction_trigger_tokens = compaction_trigger_tokens.max(1).min(window_size_tokens);
        let compaction_target_tokens = compaction_target_tokens
            .max(1)
            .min(compaction_trigger_tokens);

        Self {
            strategy,
            window_size_tokens,
            compaction_trigger_tokens,
            compaction_target_tokens,
            retrieve_top_k: retrieve_top_k.max(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSegmentKind {
    UserTurn,
    AssistantTurn,
    InjectedContext,
    Summary,
    RetrievedMemory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSegment {
    pub kind: ContextSegmentKind,
    pub token_count: usize,
    pub text: String,
}

impl ContextSegment {
    fn new(kind: ContextSegmentKind, token_count: usize, text: String) -> Self {
        Self {
            kind,
            token_count,
            text,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextState {
    pub tokens_used: usize,
    pub context_compressions: u64,
    pub context_retrieval_hits: u64,
    pub last_compaction_reason: Option<String>,
    pub last_summary_ts: Option<String>,
    pub segments: Vec<ContextSegment>,
    pub episodic_segments: Vec<ContextSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCompactionEvent {
    pub strategy: ContextStrategy,
    pub dropped_segments: usize,
    pub dropped_tokens: usize,
    pub tokens_after: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextStatusSnapshot {
    pub context_strategy: String,
    pub context_tokens_used: usize,
    pub context_window_size: usize,
    pub context_compressions: u64,
    pub context_retrieval_hits: u64,
    pub last_compaction_reason: Option<String>,
    pub last_summary_ts: Option<String>,
    pub context_segments: usize,
}

impl ContextStatusSnapshot {
    pub fn from_parts(policy: &ContextPolicy, state: &ContextState) -> Self {
        Self {
            context_strategy: policy.strategy.label().to_string(),
            context_tokens_used: state.tokens_used,
            context_window_size: policy.window_size_tokens,
            context_compressions: state.context_compressions,
            context_retrieval_hits: state.context_retrieval_hits,
            last_compaction_reason: state.last_compaction_reason.clone(),
            last_summary_ts: state.last_summary_ts.clone(),
            context_segments: state.segments.len(),
        }
    }
}

pub struct InitialContextSeed {
    pub policy: ContextPolicy,
    pub initial_segment_text: String,
}

pub struct AgentProcess {
    pub owner_id: usize,
    pub context_slot_id: Option<ContextSlotId>,
    pub state: ProcessState,
    pub model: RuntimeModel,
    pub tokenizer: Tokenizer,
    pub generation: GenerationConfig,
    pub logits_processor: LogitsProcessor,
    pub tokens: Vec<u32>,
    pub index_pos: usize,
    pub max_tokens: usize,
    pub syscall_buffer: String,
    pub context_policy: ContextPolicy,
    pub context_state: ContextState,
}

impl AgentProcess {
    pub fn new(
        id: u64,
        owner_id: usize,
        model: RuntimeModel,
        tokenizer: Tokenizer,
        prompt_tokens: Vec<u32>,
        generation: GenerationConfig,
        context_seed: InitialContextSeed,
    ) -> Self {
        let initial_token_count = prompt_tokens.len();
        AgentProcess {
            owner_id,
            context_slot_id: None,
            state: ProcessState::Ready,
            model,
            tokenizer,
            generation,
            logits_processor: LogitsProcessor::new(
                generation.seed + id,
                Some(generation.temperature),
                Some(generation.top_p),
            ),
            tokens: prompt_tokens,
            index_pos: 0,
            max_tokens: generation.max_tokens,
            syscall_buffer: String::new(),
            context_policy: context_seed.policy,
            context_state: ContextState {
                tokens_used: initial_token_count,
                context_compressions: 0,
                context_retrieval_hits: 0,
                last_compaction_reason: None,
                last_summary_ts: None,
                segments: vec![ContextSegment::new(
                    ContextSegmentKind::UserTurn,
                    initial_token_count,
                    context_seed.initial_segment_text,
                )],
                episodic_segments: Vec::new(),
            },
        }
    }

    pub fn record_model_output(&mut self, text: &str, token_count: usize) {
        self.append_segment(ContextSegmentKind::AssistantTurn, text, token_count, true);
    }

    pub fn record_injected_context(&mut self, text: &str, token_count: usize) {
        self.append_segment(ContextSegmentKind::InjectedContext, text, token_count, false);
    }

    pub fn context_status_snapshot(&self) -> ContextStatusSnapshot {
        ContextStatusSnapshot::from_parts(&self.context_policy, &self.context_state)
    }

    pub fn enforce_context_budget(&mut self) -> Option<ContextCompactionEvent> {
        self.context_state.tokens_used = self.tokens.len();

        if self.context_state.tokens_used <= self.context_policy.compaction_trigger_tokens {
            return None;
        }

        match self.context_policy.strategy {
            ContextStrategy::SlidingWindow => self.enforce_sliding_window_budget(),
            ContextStrategy::Summarize => self.enforce_summary_budget(),
            ContextStrategy::Retrieve => self.enforce_retrieve_budget(),
        }
    }

    fn enforce_retrieve_budget(&mut self) -> Option<ContextCompactionEvent> {
        let retrieved_prefix_segments = self
            .context_state
            .segments
            .iter()
            .take_while(|segment| segment.kind == ContextSegmentKind::RetrievedMemory)
            .count();
        let retrieved_prefix_tokens: usize = self
            .context_state
            .segments
            .iter()
            .take(retrieved_prefix_segments)
            .map(|segment| segment.token_count)
            .sum();

        let live_segments = &self.context_state.segments[retrieved_prefix_segments..];
        let live_tokens = self
            .context_state
            .tokens_used
            .saturating_sub(retrieved_prefix_tokens);

        let mut archived_count = 0usize;
        let mut archived_tokens = 0usize;
        while live_tokens.saturating_sub(archived_tokens) > self.context_policy.compaction_target_tokens {
            let Some(segment) = live_segments.get(archived_count) else {
                break;
            };
            if archived_count + 1 >= live_segments.len() {
                break;
            }
            archived_tokens += segment.token_count;
            archived_count += 1;
        }

        if live_tokens > self.context_policy.compaction_target_tokens
            && archived_count == 0
            && self.context_state.episodic_segments.is_empty()
            && retrieved_prefix_segments == 0
        {
            self.context_state.last_compaction_reason = Some(
                "retrieve_no_complete_segment_fit".to_string(),
            );
            return None;
        }

        let mut retrieval_corpus = self.context_state.episodic_segments.clone();
        retrieval_corpus.extend(live_segments.iter().take(archived_count).cloned());

        let base_tokens_after_archive = live_tokens.saturating_sub(archived_tokens);
        let remaining_budget = self
            .context_policy
            .window_size_tokens
            .saturating_sub(base_tokens_after_archive);
        let retrieval_payload = self.build_retrieval_payload(&retrieval_corpus, remaining_budget);

        let changed = retrieved_prefix_segments > 0 || archived_count > 0 || retrieval_payload.is_some();
        if !changed {
            return None;
        }

        self.reset_backend_context_slot("retrieve")?;

        if retrieved_prefix_segments > 0 {
            self.context_state.segments.drain(0..retrieved_prefix_segments);
            self.tokens.drain(0..retrieved_prefix_tokens);
        }

        if archived_count > 0 {
            let archived_segments: Vec<ContextSegment> = self
                .context_state
                .segments
                .drain(0..archived_count)
                .collect();
            self.context_state.episodic_segments.extend(archived_segments);
            self.tokens.drain(0..archived_tokens);
        }

        let mut retrieval_hits = 0usize;
        if let Some((text, encoded_tokens, hits)) = retrieval_payload {
            retrieval_hits = hits;
            self.context_state.segments.insert(
                0,
                ContextSegment::new(
                    ContextSegmentKind::RetrievedMemory,
                    encoded_tokens.len(),
                    text,
                ),
            );
            self.tokens.splice(0..0, encoded_tokens);
            self.context_state.context_retrieval_hits += hits as u64;
        }

        self.index_pos = 0;
        self.context_state.tokens_used = self.tokens.len();
        let reason = format!(
            "retrieve_archived_segments={} archived_tokens={} retrieval_hits={}",
            archived_count, archived_tokens, retrieval_hits
        );
        self.context_state.last_compaction_reason = Some(reason.clone());

        Some(ContextCompactionEvent {
            strategy: self.context_policy.strategy,
            dropped_segments: archived_count + retrieved_prefix_segments,
            dropped_tokens: archived_tokens + retrieved_prefix_tokens,
            tokens_after: self.tokens.len(),
            reason,
        })
    }

    fn enforce_sliding_window_budget(&mut self) -> Option<ContextCompactionEvent> {
        let mut dropped_segments = 0usize;
        let mut dropped_tokens = 0usize;
        let total_segments = self.context_state.segments.len();

        while self.context_state.tokens_used.saturating_sub(dropped_tokens)
            > self.context_policy.compaction_target_tokens
        {
            let Some(segment) = self.context_state.segments.get(dropped_segments) else {
                break;
            };
            if dropped_segments + 1 >= total_segments {
                break;
            }
            dropped_tokens += segment.token_count;
            dropped_segments += 1;
        }

        if dropped_segments == 0 || dropped_tokens == 0 {
            self.context_state.last_compaction_reason = Some(
                "sliding_window_no_complete_segment_fit".to_string(),
            );
            return None;
        }

        self.reset_backend_context_slot("sliding_window")?;

        self.context_state.segments.drain(0..dropped_segments);
        self.tokens.drain(0..dropped_tokens);
        self.index_pos = 0;
        self.context_state.tokens_used = self.tokens.len();
        self.context_state.context_compressions += 1;

        let reason = format!(
            "sliding_window_dropped_segments={} dropped_tokens={}",
            dropped_segments, dropped_tokens
        );
        self.context_state.last_compaction_reason = Some(reason.clone());

        Some(ContextCompactionEvent {
            strategy: self.context_policy.strategy,
            dropped_segments,
            dropped_tokens,
            tokens_after: self.tokens.len(),
            reason,
        })
    }

    fn enforce_summary_budget(&mut self) -> Option<ContextCompactionEvent> {
        let total_segments = self.context_state.segments.len();
        if total_segments < 2 {
            self.context_state.last_compaction_reason = Some(
                "summarize_no_complete_segment_fit".to_string(),
            );
            return None;
        }

        let mut dropped_segments = 1usize;
        let mut dropped_tokens = self.context_state.segments[0].token_count;
        let (summary_text, summary_tokens) = loop {
            let source_segments = &self.context_state.segments[..dropped_segments];
            let summary_text = build_summary_text(source_segments);
            let summary_tokens = self
                .tokenizer
                .encode(summary_text.as_str(), true)
                .ok()?
                .get_ids()
                .to_vec();

            let tokens_after = self
                .context_state
                .tokens_used
                .saturating_sub(dropped_tokens)
                .saturating_add(summary_tokens.len());
            if tokens_after <= self.context_policy.compaction_target_tokens {
                break (summary_text, summary_tokens);
            }

            if dropped_segments + 1 >= total_segments {
                self.context_state.last_compaction_reason = Some(
                    "summarize_no_complete_segment_fit".to_string(),
                );
                return None;
            }

            dropped_tokens += self.context_state.segments[dropped_segments].token_count;
            dropped_segments += 1;
        };

        self.reset_backend_context_slot("summarize")?;

        self.context_state.segments.drain(0..dropped_segments);
        self.context_state.segments.insert(
            0,
            ContextSegment::new(
                ContextSegmentKind::Summary,
                summary_tokens.len(),
                summary_text,
            ),
        );
        self.tokens.drain(0..dropped_tokens);
        self.tokens.splice(0..0, summary_tokens.iter().copied());
        self.index_pos = 0;
        self.context_state.tokens_used = self.tokens.len();
        self.context_state.context_compressions += 1;
        self.context_state.last_summary_ts = Some(crate::checkpoint::now_timestamp());

        let reason = format!(
            "summarize_compacted_segments={} replaced_tokens={}",
            dropped_segments, dropped_tokens
        );
        self.context_state.last_compaction_reason = Some(reason.clone());

        Some(ContextCompactionEvent {
            strategy: self.context_policy.strategy,
            dropped_segments,
            dropped_tokens,
            tokens_after: self.tokens.len(),
            reason,
        })
    }

    fn reset_backend_context_slot(
        &mut self,
        strategy_label: &str,
    ) -> Option<()> {
        if let Some(slot_id) = self.context_slot_id {
            if let Err(err) = self.model.free_context_slot(slot_id) {
                self.context_state.last_compaction_reason = Some(format!(
                    "{}_backend_reset_failed:{}",
                    strategy_label, err
                ));
                return None;
            }
        }

        Some(())
    }

    fn build_retrieval_payload(
        &self,
        corpus: &[ContextSegment],
        remaining_budget: usize,
    ) -> Option<(String, Vec<u32>, usize)> {
        if corpus.is_empty() || remaining_budget == 0 {
            return None;
        }

        let mut selected_texts: Vec<String> = Vec::new();
        let mut selected_hits = 0usize;

        for segment in corpus
            .iter()
            .rev()
            .take(self.context_policy.retrieve_top_k)
        {
            let candidate_text = segment.text.trim().to_string();
            if candidate_text.is_empty() {
                continue;
            }

            let mut next_texts = vec![candidate_text];
            next_texts.extend(selected_texts.iter().cloned());
            let next_text = next_texts.join("\n");
            let next_tokens = self
                .tokenizer
                .encode(next_text.as_str(), true)
                .ok()?
                .get_ids()
                .to_vec();
            if next_tokens.len() > remaining_budget {
                continue;
            }

            selected_texts = next_texts;
            selected_hits += 1;
        }

        if selected_texts.is_empty() {
            return None;
        }

        let text = selected_texts.join("\n");
        let encoded_tokens = self
            .tokenizer
            .encode(text.as_str(), true)
            .ok()?
            .get_ids()
            .to_vec();
        Some((text, encoded_tokens, selected_hits))
    }

    fn append_segment(
        &mut self,
        kind: ContextSegmentKind,
        text: &str,
        token_count: usize,
        merge_tail: bool,
    ) {
        if token_count == 0 && text.is_empty() {
            return;
        }

        if merge_tail {
            if let Some(last) = self.context_state.segments.last_mut() {
                if last.kind == kind {
                    last.token_count += token_count;
                    last.text.push_str(text);
                    self.context_state.tokens_used = self.tokens.len();
                    return;
                }
            }
        }

        self.context_state
            .segments
            .push(ContextSegment::new(kind, token_count, text.to_string()));
        self.context_state.tokens_used = self.tokens.len();
    }
}

fn build_summary_text(_segments: &[ContextSegment]) -> String {
    if _segments.is_empty() {
        return "summary".to_string();
    }

    "summary".to_string()
}

#[cfg(test)]
mod tests {
    use super::{AgentProcess, ContextPolicy, ContextSegmentKind, ContextStrategy, InitialContextSeed};
    use crate::backend::{
        ContextSlotPersistence, InferenceBackend, InferenceStepRequest, InferenceStepResult,
        RuntimeModel,
    };
    use crate::memory::ContextSlotId;
    use crate::prompting::GenerationConfig;
    use anyhow::Result;
    use std::path::Path;
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

    #[test]
    fn sliding_window_drops_complete_segments_and_resets_backend_slot() {
        let policy = ContextPolicy::new(ContextStrategy::SlidingWindow, 8, 7, 5, 3);
        let (mut process, frees) = test_process(policy);
        process.context_slot_id = Some(11);
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
        assert_eq!(process.context_state.segments[0].kind, ContextSegmentKind::AssistantTurn);
        assert_eq!(*frees.lock().expect("lock frees"), vec![11]);
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
        process.context_slot_id = Some(9);
        process.tokens.extend([5, 6, 3, 4, 4, 4]);
        process.record_injected_context("system note", 2);
        process.record_model_output("assistant reply reply reply", 4);
        process.index_pos = process.tokens.len();

        let event = process
            .enforce_context_budget()
            .expect("summary compaction should run");

        assert_eq!(event.strategy, ContextStrategy::Summarize);
        assert_eq!(process.context_state.segments[0].kind, ContextSegmentKind::Summary);
        assert!(process.context_state.last_summary_ts.is_some());
        assert!(process.tokens.len() <= process.context_policy.compaction_target_tokens);
        assert_eq!(*frees.lock().expect("lock frees"), vec![9]);
    }

    #[test]
    fn retrieve_archives_old_segments_and_reinjects_top_k_context() {
        let policy = ContextPolicy::new(ContextStrategy::Retrieve, 8, 7, 5, 2);
        let (mut process, frees) = test_process(policy);
        process.context_slot_id = Some(13);
        process.tokens.extend([5, 6, 3, 4, 4, 4]);
        process.record_injected_context("system note", 2);
        process.record_model_output("assistant reply reply reply", 4);
        process.index_pos = process.tokens.len();

        let event = process
            .enforce_context_budget()
            .expect("retrieve compaction should run");

        assert_eq!(event.strategy, ContextStrategy::Retrieve);
        assert_eq!(process.context_state.episodic_segments.len(), 2);
        assert_eq!(process.context_state.segments[0].kind, ContextSegmentKind::RetrievedMemory);
        assert_eq!(process.context_state.segments[1].kind, ContextSegmentKind::AssistantTurn);
        assert_eq!(process.context_state.context_retrieval_hits, 1);
        assert!(process.tokens.len() <= process.context_policy.window_size_tokens);
        assert_eq!(*frees.lock().expect("lock frees"), vec![13]);
    }
}
