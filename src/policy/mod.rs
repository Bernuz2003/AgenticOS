use crate::model_catalog::{infer_workload_class, parse_workload_hint, parse_workload_label, WorkloadClass};
use crate::prompting::{GenerationConfig, PromptFamily};

pub fn workload_from_label_or_default(raw: Option<&str>) -> WorkloadClass {
    raw.and_then(parse_workload_label).unwrap_or(WorkloadClass::General)
}

pub fn resolve_exec_workload(prompt_raw: &str) -> (WorkloadClass, Option<WorkloadClass>, String) {
    let (hinted_workload, prompt) = parse_workload_hint(prompt_raw);
    let workload = hinted_workload.unwrap_or_else(|| infer_workload_class(&prompt));
    (workload, hinted_workload, prompt)
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
mod tests {
    use super::{generation_defaults, resolve_exec_workload, scheduler_quota_defaults, workload_from_label_or_default};
    use crate::model_catalog::WorkloadClass;
    use crate::prompting::PromptFamily;

    #[test]
    fn workload_label_defaults_to_general() {
        assert!(matches!(workload_from_label_or_default(None), WorkloadClass::General));
        assert!(matches!(workload_from_label_or_default(Some("unknown")), WorkloadClass::General));
        assert!(matches!(workload_from_label_or_default(Some("CODE")), WorkloadClass::Code));
    }

    #[test]
    fn resolve_exec_workload_prefers_explicit_hint() {
        let (workload, hinted, prompt) = resolve_exec_workload("capability=fast; scrivi codice rust");
        assert!(matches!(workload, WorkloadClass::Fast));
        assert!(matches!(hinted, Some(WorkloadClass::Fast)));
        assert_eq!(prompt, "scrivi codice rust");
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
}