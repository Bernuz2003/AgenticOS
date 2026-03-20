use std::collections::{HashMap, HashSet};
use std::time::Instant;

use super::context::{ContextPolicy, ContextSegment, ContextSegmentKind};

#[derive(Debug, Clone)]
pub(crate) struct RankedRetrievalCandidate {
    pub idx: usize,
    pub text: String,
    pub score: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RetrievalRanking {
    pub candidates: Vec<RankedRetrievalCandidate>,
    pub candidates_scored: usize,
    pub top_score: Option<f64>,
    pub elapsed_ms: u64,
}

pub(crate) fn rank_retrieval_candidates(
    policy: &ContextPolicy,
    live_segments: &[ContextSegment],
    corpus: &[ContextSegment],
) -> RetrievalRanking {
    let started_at = Instant::now();
    if corpus.is_empty() {
        return RetrievalRanking::default();
    }

    let query_text = live_segments
        .iter()
        .filter(|segment| segment.kind != ContextSegmentKind::RetrievedMemory)
        .rev()
        .take(3)
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let query_fingerprint = SemanticFingerprint::from_text(&semantic_excerpt(
        &query_text,
        policy.retrieve_max_segment_chars.saturating_mul(2),
    ));

    let start_idx = corpus.len().saturating_sub(policy.retrieve_candidate_limit);
    let candidate_window = &corpus[start_idx..];
    let mut prepared = Vec::new();
    for (offset, segment) in candidate_window.iter().enumerate() {
        let candidate_text = semantic_excerpt(&segment.text, policy.retrieve_max_segment_chars);
        if candidate_text.is_empty() {
            continue;
        }
        let fingerprint = SemanticFingerprint::from_text(&candidate_text);
        if fingerprint.is_empty() {
            continue;
        }
        prepared.push(PreparedCandidate {
            idx: start_idx + offset,
            kind: segment.kind,
            text: candidate_text,
            fingerprint,
        });
    }

    if prepared.is_empty() {
        return RetrievalRanking {
            elapsed_ms: started_at.elapsed().as_millis() as u64,
            ..RetrievalRanking::default()
        };
    }

    let idf = build_query_idf(&query_fingerprint, &prepared);
    let candidates_scored = prepared.len();
    let total_candidates = candidates_scored.max(1);
    let mut ranked = prepared
        .into_iter()
        .filter_map(|candidate| {
            let score = semantic_score(&query_fingerprint, &idf, &candidate, total_candidates);
            (score >= policy.retrieve_min_score).then_some(RankedRetrievalCandidate {
                idx: candidate.idx,
                text: candidate.text,
                score,
            })
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.idx.cmp(&left.idx))
    });

    let top_score = ranked.first().map(|candidate| candidate.score);
    let overflow_pool = policy
        .retrieve_top_k
        .saturating_mul(4)
        .max(policy.retrieve_top_k);
    ranked.truncate(overflow_pool);

    RetrievalRanking {
        candidates: ranked,
        candidates_scored,
        top_score,
        elapsed_ms: started_at.elapsed().as_millis() as u64,
    }
}

#[derive(Debug, Clone)]
struct PreparedCandidate {
    idx: usize,
    kind: ContextSegmentKind,
    text: String,
    fingerprint: SemanticFingerprint,
}

#[derive(Debug, Clone, Default)]
struct SemanticFingerprint {
    term_set: HashSet<String>,
    phrase_set: HashSet<String>,
    trigram_set: HashSet<String>,
}

impl SemanticFingerprint {
    fn from_text(text: &str) -> Self {
        let terms = normalized_terms(text);
        let term_set = terms.iter().cloned().collect::<HashSet<_>>();
        let phrase_set = terms
            .windows(2)
            .map(|window| format!("{} {}", window[0], window[1]))
            .collect::<HashSet<_>>();
        let trigram_source = terms.join(" ");
        let trigram_set = char_trigrams(&trigram_source);

        Self {
            term_set,
            phrase_set,
            trigram_set,
        }
    }

    fn is_empty(&self) -> bool {
        self.term_set.is_empty() && self.phrase_set.is_empty() && self.trigram_set.is_empty()
    }
}

fn semantic_score(
    query: &SemanticFingerprint,
    idf: &HashMap<String, f64>,
    candidate: &PreparedCandidate,
    total_candidates: usize,
) -> f64 {
    let term_overlap = weighted_overlap(query, &candidate.fingerprint, idf);
    let phrase_overlap = set_overlap_ratio(&query.phrase_set, &candidate.fingerprint.phrase_set);
    let trigram_similarity = set_jaccard(&query.trigram_set, &candidate.fingerprint.trigram_set);
    let recency_bonus = (candidate.idx + 1) as f64 / total_candidates as f64;
    let kind_weight = match candidate.kind {
        ContextSegmentKind::UserTurn | ContextSegmentKind::AssistantTurn => 1.0,
        ContextSegmentKind::InjectedContext => 0.9,
        ContextSegmentKind::Summary => 0.88,
        ContextSegmentKind::RetrievedMemory => 0.82,
    };

    kind_weight * ((term_overlap * 0.62) + (phrase_overlap * 0.23) + (trigram_similarity * 0.10))
        + (recency_bonus * 0.05)
}

fn build_query_idf(
    query: &SemanticFingerprint,
    prepared: &[PreparedCandidate],
) -> HashMap<String, f64> {
    let doc_count = prepared.len() as f64;
    query
        .term_set
        .iter()
        .map(|term| {
            let document_frequency = prepared
                .iter()
                .filter(|candidate| candidate.fingerprint.term_set.contains(term))
                .count() as f64;
            let idf = ((doc_count + 1.0) / (document_frequency + 1.0)).ln() + 1.0;
            (term.clone(), idf)
        })
        .collect()
}

fn weighted_overlap(
    query: &SemanticFingerprint,
    candidate: &SemanticFingerprint,
    idf: &HashMap<String, f64>,
) -> f64 {
    if query.term_set.is_empty() {
        return 0.0;
    }

    let total_weight = query
        .term_set
        .iter()
        .map(|term| idf.get(term).copied().unwrap_or(1.0))
        .sum::<f64>();
    if total_weight <= f64::EPSILON {
        return 0.0;
    }

    let matched_weight = query
        .term_set
        .iter()
        .filter(|term| candidate.term_set.contains(*term))
        .map(|term| idf.get(term).copied().unwrap_or(1.0))
        .sum::<f64>();
    matched_weight / total_weight
}

fn set_overlap_ratio(query: &HashSet<String>, candidate: &HashSet<String>) -> f64 {
    if query.is_empty() {
        return 0.0;
    }
    let overlap = query
        .iter()
        .filter(|item| candidate.contains(*item))
        .count() as f64;
    overlap / query.len() as f64
}

fn set_jaccard(left: &HashSet<String>, right: &HashSet<String>) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.iter().filter(|item| right.contains(*item)).count() as f64;
    let union = (left.len() + right.len()) as f64 - intersection;
    if union <= f64::EPSILON {
        0.0
    } else {
        intersection / union
    }
}

fn semantic_excerpt(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() || max_chars == 0 {
        return String::new();
    }
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let head_chars = max_chars / 2;
    let tail_chars = max_chars.saturating_sub(head_chars);
    let head = trimmed.chars().take(head_chars).collect::<String>();
    let tail = trimmed
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{head}\n...\n{tail}")
}

fn normalized_terms(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter_map(normalize_term)
        .collect()
}

fn normalize_term(raw: &str) -> Option<String> {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.len() < 2 || is_stopword(&lower) {
        return None;
    }

    let stemmed = stem_term(&lower);
    (stemmed.len() >= 2).then_some(stemmed)
}

fn stem_term(term: &str) -> String {
    let mut stem = term.to_string();
    for (suffix, replacement, min_len) in [
        ("azioni", "azione", 8),
        ("ments", "ment", 7),
        ("menti", "ment", 7),
        ("zione", "zion", 6),
        ("zioni", "zion", 6),
        ("ingly", "ing", 6),
        ("edly", "ed", 5),
        ("ing", "", 5),
        ("ers", "er", 5),
        ("ies", "y", 5),
        ("ied", "y", 5),
        ("ed", "", 4),
        ("ly", "", 4),
        ("es", "", 4),
        ("s", "", 4),
    ] {
        if stem.len() >= min_len && stem.ends_with(suffix) {
            stem.truncate(stem.len() - suffix.len());
            stem.push_str(replacement);
            break;
        }
    }
    stem
}

fn is_stopword(term: &str) -> bool {
    matches!(
        term,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "by"
            | "da"
            | "de"
            | "del"
            | "della"
            | "di"
            | "ed"
            | "e"
            | "for"
            | "gli"
            | "i"
            | "il"
            | "in"
            | "is"
            | "it"
            | "la"
            | "le"
            | "lo"
            | "nel"
            | "nella"
            | "of"
            | "on"
            | "or"
            | "per"
            | "su"
            | "that"
            | "the"
            | "these"
            | "this"
            | "those"
            | "to"
            | "un"
            | "una"
            | "uno"
            | "with"
            | "your"
    )
}

fn char_trigrams(text: &str) -> HashSet<String> {
    let condensed = text
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let chars = condensed.chars().collect::<Vec<_>>();
    if chars.len() < 3 {
        return HashSet::new();
    }

    chars
        .windows(3)
        .map(|window| window.iter().collect::<String>())
        .collect()
}
