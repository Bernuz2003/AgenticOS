/// The core AgentProcess structure and budget enforcement algorithms.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokenizers::Tokenizer;

use super::context::*;
use super::state::*;
use crate::backend::RuntimeModel;
use crate::memory::ContextSlotId;
use crate::prompting::GenerationConfig;
use crate::tools::invocation::ToolCaller;

pub struct AgentProcess {
    pub owner_id: usize,
    pub tool_caller: ToolCaller,
    pub context_slot_id: Option<ContextSlotId>,
    pub resident_slot_policy: ResidentSlotPolicy,
    pub resident_slot_state: ResidentSlotState,
    pub resident_slot_snapshot_path: Option<PathBuf>,
    pub state: ProcessState,
    pub lifecycle_policy: ProcessLifecyclePolicy,
    pub model: RuntimeModel,
    pub tokenizer: Tokenizer,
    pub generation: GenerationConfig,
    pub tokens: Vec<u32>,
    pub index_pos: usize,
    pub turn_start_index: usize,
    pub max_tokens: usize,
    pub syscall_buffer: String,
    pub context_policy: ContextPolicy,
    pub context_state: ContextState,
    rendered_prompt_cache: String,
    resident_prompt_checkpoint_bytes: usize,
}

impl AgentProcess {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        _id: u64,
        owner_id: usize,
        tool_caller: ToolCaller,
        lifecycle_policy: ProcessLifecyclePolicy,
        model: RuntimeModel,
        tokenizer: Tokenizer,
        prompt_tokens: Vec<u32>,
        generation: GenerationConfig,
        context_seed: InitialContextSeed,
    ) -> Self {
        let initial_token_count = prompt_tokens.len();
        let initial_segment_text = context_seed.initial_segment_text;
        AgentProcess {
            owner_id,
            tool_caller,
            context_slot_id: None,
            resident_slot_policy: ResidentSlotPolicy::Unmanaged,
            resident_slot_state: ResidentSlotState::Unbound,
            resident_slot_snapshot_path: None,
            state: ProcessState::Ready,
            lifecycle_policy,
            model,
            tokenizer,
            generation,
            tokens: prompt_tokens,
            index_pos: 0,
            turn_start_index: initial_token_count,
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
                    initial_segment_text.clone(),
                )],
                episodic_segments: Vec::new(),
            },
            rendered_prompt_cache: initial_segment_text,
            resident_prompt_checkpoint_bytes: 0,
        }
    }

    pub fn record_model_output(&mut self, text: &str, token_count: usize) {
        self.append_segment(ContextSegmentKind::AssistantTurn, text, token_count, true);
    }

    pub fn record_user_input(&mut self, text: &str, token_count: usize) {
        self.append_segment(ContextSegmentKind::UserTurn, text, token_count, false);
    }

    pub fn record_injected_context(&mut self, text: &str, token_count: usize) {
        self.append_segment(
            ContextSegmentKind::InjectedContext,
            text,
            token_count,
            false,
        );
    }

    pub fn context_status_snapshot(&self) -> ContextStatusSnapshot {
        ContextStatusSnapshot::from_parts(&self.context_policy, &self.context_state)
    }

    pub fn prompt_text(&self) -> &str {
        &self.rendered_prompt_cache
    }

    pub fn pending_resident_prompt_suffix(&self) -> &str {
        let start = self
            .resident_prompt_checkpoint_bytes
            .min(self.rendered_prompt_cache.len());
        &self.rendered_prompt_cache[start..]
    }

    pub fn mark_resident_prompt_checkpoint(&mut self) {
        self.resident_prompt_checkpoint_bytes = self.rendered_prompt_cache.len();
    }

    pub fn reset_resident_prompt_checkpoint(&mut self) {
        self.resident_prompt_checkpoint_bytes = 0;
    }

    pub fn bind_context_slot(&mut self, slot_id: ContextSlotId, policy: ResidentSlotPolicy) {
        self.context_slot_id = Some(slot_id);
        self.resident_slot_policy = policy;
        self.resident_slot_state = ResidentSlotState::Allocated;
        self.reset_resident_prompt_checkpoint();
    }

    pub fn mark_resident_slot_park_requested(&mut self) {
        if self.context_slot_id.is_none() {
            return;
        }
        self.resident_slot_state = ResidentSlotState::ParkRequested;
    }

    pub fn mark_resident_slot_snapshot_saved(&mut self, path: PathBuf) {
        if self.context_slot_id.is_none() {
            return;
        }
        self.resident_slot_state = ResidentSlotState::SnapshotSaved;
        self.resident_slot_snapshot_path = Some(path);
    }

    pub fn mark_resident_slot_restoring(&mut self, path: PathBuf) {
        if self.context_slot_id.is_none() {
            return;
        }
        self.resident_slot_state = ResidentSlotState::Restoring;
        self.resident_slot_snapshot_path = Some(path);
    }

    pub fn mark_resident_slot_allocated(&mut self) {
        if self.context_slot_id.is_none() {
            self.resident_slot_policy = ResidentSlotPolicy::Unmanaged;
            self.resident_slot_state = ResidentSlotState::Unbound;
            return;
        }
        self.resident_slot_state = ResidentSlotState::Allocated;
    }

    pub fn resident_slot_policy_label(&self) -> Option<String> {
        self.context_slot_id
            .map(|_| self.resident_slot_policy.as_str().to_string())
    }

    pub fn resident_slot_state_label(&self) -> Option<String> {
        self.context_slot_id
            .map(|_| self.resident_slot_state.as_str().to_string())
    }

    pub fn resident_slot_snapshot_path(&self) -> Option<&Path> {
        self.resident_slot_snapshot_path.as_deref()
    }

    pub fn generated_tokens_in_current_turn(&self) -> usize {
        self.tokens.len().saturating_sub(self.turn_start_index)
    }

    pub fn begin_next_turn(&mut self) {
        self.turn_start_index = self.tokens.len();
        self.syscall_buffer.clear();
    }

    pub fn extend_current_turn_budget(&mut self) {
        self.turn_start_index = self.tokens.len();
    }

    pub fn abandon_current_turn(&mut self) {
        self.syscall_buffer.clear();
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
        while live_tokens.saturating_sub(archived_tokens)
            > self.context_policy.compaction_target_tokens
        {
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
            self.context_state.last_compaction_reason =
                Some("retrieve_no_complete_segment_fit".to_string());
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

        let changed =
            retrieved_prefix_segments > 0 || archived_count > 0 || retrieval_payload.is_some();
        if !changed {
            return None;
        }

        self.reset_backend_context_slot("retrieve")?;

        if retrieved_prefix_segments > 0 {
            self.context_state
                .segments
                .drain(0..retrieved_prefix_segments);
            self.tokens.drain(0..retrieved_prefix_tokens);
        }

        if archived_count > 0 {
            let archived_segments: Vec<ContextSegment> = self
                .context_state
                .segments
                .drain(0..archived_count)
                .collect();
            self.context_state
                .episodic_segments
                .extend(archived_segments);
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
        self.rebuild_rendered_prompt_cache();
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

        while self
            .context_state
            .tokens_used
            .saturating_sub(dropped_tokens)
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
            self.context_state.last_compaction_reason =
                Some("sliding_window_no_complete_segment_fit".to_string());
            return None;
        }

        self.reset_backend_context_slot("sliding_window")?;

        self.context_state.segments.drain(0..dropped_segments);
        self.tokens.drain(0..dropped_tokens);
        self.index_pos = 0;
        self.context_state.tokens_used = self.tokens.len();
        self.context_state.context_compressions += 1;
        self.rebuild_rendered_prompt_cache();

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
            self.context_state.last_compaction_reason =
                Some("summarize_no_complete_segment_fit".to_string());
            return None;
        }

        let mut dropped_segments = 1usize;
        let mut dropped_tokens = self.context_state.segments[0].token_count;
        let (summary_text, summary_tokens) = loop {
            let source_segments = &self.context_state.segments[..dropped_segments];
            let preserved_tokens = self
                .context_state
                .tokens_used
                .saturating_sub(dropped_tokens);
            let available_summary_tokens = self
                .context_policy
                .compaction_target_tokens
                .saturating_sub(preserved_tokens);
            if available_summary_tokens == 0 {
                if dropped_segments + 1 >= total_segments {
                    self.context_state.last_compaction_reason =
                        Some("summarize_no_complete_segment_fit".to_string());
                    return None;
                }

                dropped_tokens += self.context_state.segments[dropped_segments].token_count;
                dropped_segments += 1;
                continue;
            }

            let summary_text = build_summary_text(source_segments, available_summary_tokens);
            let summary_tokens = self
                .tokenizer
                .encode(summary_text.as_str(), true)
                .ok()?
                .get_ids()
                .to_vec();

            if !summary_tokens.is_empty() && summary_tokens.len() <= available_summary_tokens {
                break (summary_text, summary_tokens);
            }

            if dropped_segments + 1 >= total_segments {
                self.context_state.last_compaction_reason =
                    Some("summarize_no_complete_segment_fit".to_string());
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
        self.rebuild_rendered_prompt_cache();

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

    fn reset_backend_context_slot(&mut self, strategy_label: &str) -> Option<()> {
        if let Some(slot_id) = self.context_slot_id {
            if let Err(err) = self.model.free_context_slot(slot_id) {
                self.context_state.last_compaction_reason =
                    Some(format!("{}_backend_reset_failed:{}", strategy_label, err));
                return None;
            }
        }

        self.reset_resident_prompt_checkpoint();

        Some(())
    }

    pub(crate) fn build_retrieval_payload(
        &self,
        corpus: &[ContextSegment],
        remaining_budget: usize,
    ) -> Option<(String, Vec<u32>, usize)> {
        if corpus.is_empty() || remaining_budget == 0 {
            return None;
        }

        let live_query = self
            .context_state
            .segments
            .iter()
            .filter(|segment| segment.kind != ContextSegmentKind::RetrievedMemory)
            .rev()
            .take(3)
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let query_terms = lexical_terms(&live_query);

        let mut ranked: Vec<(usize, usize, String)> = corpus
            .iter()
            .enumerate()
            .filter_map(|(idx, segment)| {
                let candidate_text = segment.text.trim().to_string();
                if candidate_text.is_empty() {
                    return None;
                }
                let overlap = lexical_overlap_score(&query_terms, &candidate_text);
                let recency_bonus = idx + 1;
                let score = overlap.saturating_mul(100) + recency_bonus;
                Some((score, idx, candidate_text))
            })
            .collect();
        ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));

        let mut chosen: Vec<(usize, String)> = ranked
            .into_iter()
            .take(self.context_policy.retrieve_top_k)
            .map(|(_, idx, text)| (idx, text))
            .collect();
        chosen.sort_by_key(|(idx, _)| *idx);

        let mut selected_texts: Vec<String> = Vec::new();
        let mut selected_hits = 0usize;

        for (_idx, candidate_text) in chosen {
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
                    self.rendered_prompt_cache.push_str(text);
                    self.context_state.tokens_used = self.tokens.len();
                    return;
                }
            }
        }

        self.context_state
            .segments
            .push(ContextSegment::new(kind, token_count, text.to_string()));
        self.rendered_prompt_cache.push_str(text);
        self.context_state.tokens_used = self.tokens.len();
    }

    pub(crate) fn rebuild_rendered_prompt_cache(&mut self) {
        self.rendered_prompt_cache.clear();
        for segment in &self.context_state.segments {
            self.rendered_prompt_cache.push_str(&segment.text);
        }
        self.resident_prompt_checkpoint_bytes = self
            .resident_prompt_checkpoint_bytes
            .min(self.rendered_prompt_cache.len());
    }
}

fn build_summary_text(segments: &[ContextSegment], max_summary_tokens: usize) -> String {
    if segments.is_empty() || max_summary_tokens == 0 {
        return "Summary of earlier context: no retained details.".to_string();
    }

    let include_labels = max_summary_tokens >= 4;
    let mut terms = Vec::new();

    'segments: for segment in segments {
        if include_labels {
            let label = match segment.kind {
                ContextSegmentKind::UserTurn => "user",
                ContextSegmentKind::AssistantTurn => "assistant",
                ContextSegmentKind::InjectedContext => "system",
                ContextSegmentKind::Summary => "summary",
                ContextSegmentKind::RetrievedMemory => "memory",
            };
            if terms.last().map(|term: &String| term.as_str()) != Some(label) {
                terms.push(label.to_string());
            }
            if terms.len() >= max_summary_tokens {
                break;
            }
        }

        for word in segment.text.split(|ch: char| !ch.is_alphanumeric()) {
            let normalized = word.trim();
            if normalized.is_empty() {
                continue;
            }
            let normalized = normalized.to_ascii_lowercase();
            if terms.last() == Some(&normalized) {
                continue;
            }
            terms.push(normalized);
            if terms.len() >= max_summary_tokens {
                break 'segments;
            }
        }
    }

    if terms.is_empty() {
        return "summary".to_string();
    }

    if terms.len() == 1 {
        return terms.pop().unwrap_or_else(|| "summary".to_string());
    }

    let mut summary = terms.join(" ");
    if summary.is_empty() {
        return "summary".to_string();
    }

    if include_labels && terms.len() < max_summary_tokens {
        summary.insert_str(0, "summary ");
    }
    summary
}

fn lexical_terms(text: &str) -> HashSet<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|term| term.len() >= 3)
        .map(|term| term.to_ascii_lowercase())
        .collect()
}

fn lexical_overlap_score(query_terms: &HashSet<String>, candidate_text: &str) -> usize {
    if query_terms.is_empty() {
        return 0;
    }
    lexical_terms(candidate_text)
        .into_iter()
        .filter(|term| query_terms.contains(term))
        .count()
}
