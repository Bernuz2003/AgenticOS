use super::{
    generation_defaults, resolve_exec_policy, scheduler_quota_defaults,
    workload_from_label_or_default,
};
use crate::model_catalog::WorkloadClass;
use crate::process::ContextStrategy;
use crate::prompting::PromptFamily;

#[test]
fn workload_label_defaults_to_general() {
    assert!(matches!(
        workload_from_label_or_default(None),
        WorkloadClass::General
    ));
    assert!(matches!(
        workload_from_label_or_default(Some("unknown")),
        WorkloadClass::General
    ));
    assert!(matches!(
        workload_from_label_or_default(Some("CODE")),
        WorkloadClass::Code
    ));
}

#[test]
fn resolve_exec_policy_parses_context_overrides_additively() {
    let resolved = resolve_exec_policy(
            "capability=code; context_strategy=sliding; context_window=300; context_trigger=250; context_target=200; scrivi un parser rust",
        );

    assert!(matches!(resolved.workload, WorkloadClass::Code));
    assert!(matches!(
        resolved.hinted_workload,
        Some(WorkloadClass::Code)
    ));
    assert_eq!(resolved.prompt, "scrivi un parser rust");
    assert_eq!(
        resolved.context_policy.strategy,
        ContextStrategy::SlidingWindow
    );
    assert_eq!(resolved.context_policy.window_size_tokens, 300);
    assert_eq!(resolved.context_policy.compaction_trigger_tokens, 250);
    assert_eq!(resolved.context_policy.compaction_target_tokens, 200);
}

#[test]
fn resolve_exec_policy_leaves_unknown_prefix_in_prompt() {
    let resolved = resolve_exec_policy("context_mode=weird; capability=fast; hello world");

    assert_eq!(
        resolved.prompt,
        "context_mode=weird; capability=fast; hello world"
    );
}

#[test]
fn scheduler_quota_defaults_vary_by_workload() {
    let fast = scheduler_quota_defaults(WorkloadClass::Fast);
    let reasoning = scheduler_quota_defaults(WorkloadClass::Reasoning);
    assert_ne!(fast, reasoning);
}

#[test]
fn generation_defaults_delegate_to_prompt_family_profiles() {
    let llama = generation_defaults(PromptFamily::Llama);
    let qwen = generation_defaults(PromptFamily::Qwen);
    assert!(llama.max_tokens > 0);
    assert!(qwen.max_tokens > 0);
}
