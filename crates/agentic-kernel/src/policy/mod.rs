use crate::model_catalog::{infer_workload_class, parse_workload_label, WorkloadClass};
use crate::process::{ContextPolicy, ContextStrategy};
use crate::prompting::{GenerationConfig, PromptFamily};

pub struct ExecResolvedPolicy {
    pub workload: WorkloadClass,
    pub hinted_workload: Option<WorkloadClass>,
    pub prompt: String,
    pub context_policy: ContextPolicy,
}

pub fn workload_from_label_or_default(raw: Option<&str>) -> WorkloadClass {
    raw.and_then(parse_workload_label)
        .unwrap_or(WorkloadClass::General)
}

pub fn resolve_exec_policy(prompt_raw: &str) -> ExecResolvedPolicy {
    let (hinted_workload, context_policy, prompt) = parse_exec_prefix_overrides(prompt_raw);
    let workload = hinted_workload.unwrap_or_else(|| infer_workload_class(&prompt));
    ExecResolvedPolicy {
        workload,
        hinted_workload,
        prompt,
        context_policy,
    }
}

fn parse_exec_prefix_overrides(prompt_raw: &str) -> (Option<WorkloadClass>, ContextPolicy, String) {
    let trimmed = prompt_raw.trim_start();
    let mut consumed_until = 0usize;
    let mut workload_hint = None;
    let mut strategy_override = None;
    let mut window_override = None;
    let mut trigger_override = None;
    let mut target_override = None;
    let mut retrieve_override = None;

    for segment in trimmed.split_inclusive(';') {
        let candidate = segment.trim_end_matches(';').trim();
        let Some((raw_key, raw_value)) = candidate.split_once('=') else {
            break;
        };

        let key = raw_key.trim().to_ascii_lowercase();
        let value = raw_value.trim();

        let recognized = match key.as_str() {
            "capability" => {
                workload_hint = parse_workload_label(value);
                true
            }
            "context_strategy" => {
                strategy_override = ContextStrategy::parse(value);
                true
            }
            "context_window" | "context_window_tokens" => {
                window_override = value.parse::<usize>().ok();
                true
            }
            "context_trigger" | "context_trigger_tokens" => {
                trigger_override = value.parse::<usize>().ok();
                true
            }
            "context_target" | "context_target_tokens" => {
                target_override = value.parse::<usize>().ok();
                true
            }
            "context_retrieve_top_k" => {
                retrieve_override = value.parse::<usize>().ok();
                true
            }
            _ => false,
        };

        if !recognized {
            break;
        }

        consumed_until += segment.len();
    }

    let prompt = if consumed_until == 0 {
        prompt_raw.to_string()
    } else {
        trimmed[consumed_until..].trim_start().to_string()
    };

    let defaults = crate::config::kernel_config().context.clone();
    let strategy = strategy_override
        .or_else(|| ContextStrategy::parse(&defaults.default_strategy))
        .unwrap_or_default();
    let context_policy = ContextPolicy::new(
        strategy,
        window_override.unwrap_or(defaults.default_window_tokens),
        trigger_override.unwrap_or(defaults.compaction_trigger_tokens),
        target_override.unwrap_or(defaults.compaction_target_tokens),
        retrieve_override.unwrap_or(defaults.retrieve_top_k),
    );

    (workload_hint, context_policy, prompt)
}

pub fn generation_defaults(family: PromptFamily) -> GenerationConfig {
    GenerationConfig::defaults_for(family)
}

pub fn scheduler_quota_defaults(workload: WorkloadClass) -> (usize, usize) {
    let scheduler = &crate::config::kernel_config().scheduler;
    let quota = match workload {
        WorkloadClass::Fast => scheduler.fast,
        WorkloadClass::Code => scheduler.code,
        WorkloadClass::Reasoning => scheduler.reasoning,
        WorkloadClass::General => scheduler.general,
    };

    (quota.max_tokens, quota.max_syscalls)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
