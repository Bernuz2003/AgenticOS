/// Context segments and token budget tracking logic.
///
/// **Context Budgeting Math Rules:**
/// The context window defines a strict upper bound (`window_size_tokens`). When the
/// prompt size exceeds `compaction_trigger_tokens`, a compaction strategy is invoked
/// (SlidingWindow, Summarize, Retrieve). Compaction must forcefully reduce the token
/// footprint to at or below `compaction_target_tokens` to ensure the next generation
/// cycle has ample headroom for the model's output without risking context overflow.
use serde::{Deserialize, Serialize};

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
#[serde(default)]
pub struct ContextPolicy {
    pub strategy: ContextStrategy,
    pub window_size_tokens: usize,
    pub compaction_trigger_tokens: usize,
    pub compaction_target_tokens: usize,
    pub retrieve_top_k: usize,
    pub retrieve_candidate_limit: usize,
    pub retrieve_max_segment_chars: usize,
    pub retrieve_min_score: f64,
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
        let config = &crate::config::kernel_config().context;
        let retrieve_top_k = retrieve_top_k.max(1);
        let retrieve_candidate_limit = config.retrieve_candidate_limit.max(retrieve_top_k);
        let retrieve_max_segment_chars = config.retrieve_max_segment_chars.max(64);
        let retrieve_min_score = config.retrieve_min_score.max(0.0);

        Self {
            strategy,
            window_size_tokens,
            compaction_trigger_tokens,
            compaction_target_tokens,
            retrieve_top_k,
            retrieve_candidate_limit,
            retrieve_max_segment_chars,
            retrieve_min_score,
        }
    }
}

impl Default for ContextPolicy {
    fn default() -> Self {
        Self::from_kernel_defaults()
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
    pub(crate) fn new(kind: ContextSegmentKind, token_count: usize, text: String) -> Self {
        Self {
            kind,
            token_count,
            text,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ContextState {
    pub tokens_used: usize,
    pub context_compressions: u64,
    pub context_retrieval_hits: u64,
    pub context_retrieval_requests: u64,
    pub context_retrieval_misses: u64,
    pub context_retrieval_candidates_scored: u64,
    pub context_retrieval_segments_selected: u64,
    pub last_retrieval_candidates_scored: usize,
    pub last_retrieval_segments_selected: usize,
    pub last_retrieval_latency_ms: u64,
    pub last_retrieval_top_score: Option<f64>,
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
    pub context_retrieval_requests: u64,
    pub context_retrieval_misses: u64,
    pub context_retrieval_candidates_scored: u64,
    pub context_retrieval_segments_selected: u64,
    pub last_retrieval_candidates_scored: usize,
    pub last_retrieval_segments_selected: usize,
    pub last_retrieval_latency_ms: u64,
    pub last_retrieval_top_score: Option<f64>,
    pub last_compaction_reason: Option<String>,
    pub last_summary_ts: Option<String>,
    pub context_segments: usize,
    pub episodic_segments: usize,
    pub episodic_tokens: usize,
    pub retrieve_top_k: usize,
    pub retrieve_candidate_limit: usize,
    pub retrieve_max_segment_chars: usize,
    pub retrieve_min_score: f64,
}

impl ContextStatusSnapshot {
    pub fn from_parts(policy: &ContextPolicy, state: &ContextState) -> Self {
        let episodic_tokens = state
            .episodic_segments
            .iter()
            .map(|segment| segment.token_count)
            .sum();
        Self {
            context_strategy: policy.strategy.label().to_string(),
            context_tokens_used: state.tokens_used,
            context_window_size: policy.window_size_tokens,
            context_compressions: state.context_compressions,
            context_retrieval_hits: state.context_retrieval_hits,
            context_retrieval_requests: state.context_retrieval_requests,
            context_retrieval_misses: state.context_retrieval_misses,
            context_retrieval_candidates_scored: state.context_retrieval_candidates_scored,
            context_retrieval_segments_selected: state.context_retrieval_segments_selected,
            last_retrieval_candidates_scored: state.last_retrieval_candidates_scored,
            last_retrieval_segments_selected: state.last_retrieval_segments_selected,
            last_retrieval_latency_ms: state.last_retrieval_latency_ms,
            last_retrieval_top_score: state.last_retrieval_top_score,
            last_compaction_reason: state.last_compaction_reason.clone(),
            last_summary_ts: state.last_summary_ts.clone(),
            context_segments: state.segments.len(),
            episodic_segments: state.episodic_segments.len(),
            episodic_tokens,
            retrieve_top_k: policy.retrieve_top_k,
            retrieve_candidate_limit: policy.retrieve_candidate_limit,
            retrieve_max_segment_chars: policy.retrieve_max_segment_chars,
            retrieve_min_score: policy.retrieve_min_score,
        }
    }
}

pub struct InitialContextSeed {
    pub policy: ContextPolicy,
    pub initial_segment_text: String,
}
