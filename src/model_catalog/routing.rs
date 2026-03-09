use crate::prompting::PromptFamily;

use super::workload::{workload_key, WorkloadClass};
use super::ModelEntry;

pub(super) struct RoutingDecision<'a> {
    pub(super) entry: Option<&'a ModelEntry>,
    pub(super) source: &'static str,
    pub(super) rationale: String,
    pub(super) capability_key: Option<&'static str>,
    pub(super) capability_score: Option<f64>,
}

pub(super) fn select_for_workload(entries: &[ModelEntry], class: WorkloadClass) -> Option<&ModelEntry> {
    recommend_for_workload(entries, class).entry
}

pub(super) fn recommend_for_workload(entries: &[ModelEntry], class: WorkloadClass) -> RoutingDecision<'_> {
    if entries.is_empty() {
        return RoutingDecision {
            entry: None,
            source: "none",
            rationale: "no models available in catalog".to_string(),
            capability_key: None,
            capability_score: None,
        };
    }

    let capability_key = workload_key(class);
    let mut scored: Vec<(&ModelEntry, f64, usize)> = entries
        .iter()
        .filter_map(|entry| {
            let score = entry
                .metadata
                .as_ref()
                .and_then(|meta| meta.capabilities.as_ref())
                .and_then(|caps| caps.get(capability_key))
                .copied()?;
            Some((entry, score, model_size_hint(&entry.id)))
        })
        .collect();
    if !scored.is_empty() {
        scored.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.2.cmp(&right.2))
        });
        if let Some((entry, score, _)) = scored.first().copied() {
            return RoutingDecision {
                entry: Some(entry),
                source: "metadata-capability",
                rationale: format!(
                    "selected by metadata capability '{}' with score {:.2}",
                    capability_key, score
                ),
                capability_key: Some(capability_key),
                capability_score: Some(score),
            };
        }
    }

    let family_order: &[PromptFamily] = match class {
        WorkloadClass::Fast => &[PromptFamily::Llama, PromptFamily::Qwen, PromptFamily::Mistral],
        WorkloadClass::Code => &[PromptFamily::Qwen, PromptFamily::Llama, PromptFamily::Mistral],
        WorkloadClass::Reasoning => &[PromptFamily::Qwen, PromptFamily::Llama, PromptFamily::Mistral],
        WorkloadClass::General => &[PromptFamily::Llama, PromptFamily::Qwen, PromptFamily::Mistral],
    };

    for family in family_order {
        let mut candidates: Vec<&ModelEntry> = entries
            .iter()
            .filter(|entry| entry.family == *family)
            .collect();

        if candidates.is_empty() {
            continue;
        }

        candidates.sort_by_key(|entry| model_size_hint(&entry.id));

        let selected = match class {
            WorkloadClass::Fast => candidates.first().copied(),
            WorkloadClass::Code | WorkloadClass::Reasoning => candidates.last().copied(),
            WorkloadClass::General => candidates.first().copied(),
        };

        if let Some(entry) = selected {
            return RoutingDecision {
                entry: Some(entry),
                source: "family-fallback",
                rationale: format!(
                    "selected by family fallback '{:?}' for '{}' workload",
                    family, capability_key
                ),
                capability_key: None,
                capability_score: None,
            };
        }
    }

    RoutingDecision {
        entry: entries.first(),
        source: "first-available",
        rationale: "selected first available model because no capability or family match applied"
            .to_string(),
        capability_key: None,
        capability_score: None,
    }
}

fn model_size_hint(model_id: &str) -> usize {
    let lower = model_id.to_lowercase();
    let mut digits = String::new();
    for ch in lower.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if !digits.is_empty() {
            break;
        }
    }

    if digits.is_empty() {
        0
    } else {
        digits.parse::<usize>().unwrap_or(0)
    }
}