#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum WorkloadClass {
    Fast,
    Code,
    Reasoning,
    #[default]
    General,
}

pub fn infer_workload_class(prompt: &str) -> WorkloadClass {
    let lowered = prompt.to_lowercase();
    if lowered.contains("python")
        || lowered.contains("rust")
        || lowered.contains("codice")
        || lowered.contains("debug")
        || lowered.contains("refactor")
    {
        WorkloadClass::Code
    } else if lowered.contains("ragiona")
        || lowered.contains("reason")
        || lowered.contains("analizza")
        || lowered.contains("dimostra")
    {
        WorkloadClass::Reasoning
    } else if lowered.contains("breve")
        || lowered.contains("short")
        || lowered.contains("riassumi")
        || lowered.contains("ping")
    {
        WorkloadClass::Fast
    } else {
        WorkloadClass::General
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn parse_workload_hint(prompt: &str) -> (Option<WorkloadClass>, String) {
    let trimmed = prompt.trim_start();
    let lower = trimmed.to_lowercase();
    let prefix = "capability=";

    if !lower.starts_with(prefix) {
        return (None, prompt.to_string());
    }

    let Some(sep_idx) = trimmed.find(';') else {
        return (None, prompt.to_string());
    };

    let hint = trimmed[prefix.len()..sep_idx].trim().to_lowercase();
    let workload = parse_workload_label(&hint);

    let stripped = trimmed[sep_idx + 1..].trim_start().to_string();
    (workload, stripped)
}

pub fn parse_workload_label(raw: &str) -> Option<WorkloadClass> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "fast" => Some(WorkloadClass::Fast),
        "code" => Some(WorkloadClass::Code),
        "reasoning" => Some(WorkloadClass::Reasoning),
        "general" => Some(WorkloadClass::General),
        _ => None,
    }
}

pub(super) fn workload_key(class: WorkloadClass) -> &'static str {
    match class {
        WorkloadClass::Fast => "fast",
        WorkloadClass::Code => "code",
        WorkloadClass::Reasoning => "reasoning",
        WorkloadClass::General => "general",
    }
}