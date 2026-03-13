use crate::runtimes::RuntimeRegistry;
use crate::session::SessionRegistry;
use crate::storage::{NewAuditEvent, StorageService};

#[derive(Debug, Clone, Copy)]
pub(crate) struct AuditSpec {
    pub(crate) category: &'static str,
    pub(crate) kind: &'static str,
    pub(crate) title: &'static str,
}

pub(crate) const ADMISSION_RESERVED: AuditSpec = AuditSpec {
    category: "admission",
    kind: "reserved",
    title: "Resources reserved",
};
pub(crate) const ADMISSION_QUEUED: AuditSpec = AuditSpec {
    category: "admission",
    kind: "queued",
    title: "Load queued",
};
pub(crate) const ADMISSION_DENIED: AuditSpec = AuditSpec {
    category: "admission",
    kind: "denied",
    title: "Load denied",
};
pub(crate) const ADMISSION_EVICTION_STARTED: AuditSpec = AuditSpec {
    category: "admission",
    kind: "eviction_started",
    title: "Eviction planned",
};
pub(crate) const ADMISSION_LOADER_BUSY: AuditSpec = AuditSpec {
    category: "admission",
    kind: "loader_busy",
    title: "Loader busy",
};

pub(crate) const RUNTIME_LOAD_STARTED: AuditSpec = AuditSpec {
    category: "runtime",
    kind: "load_started",
    title: "Load started",
};
pub(crate) const RUNTIME_LOAD_READY: AuditSpec = AuditSpec {
    category: "runtime",
    kind: "load_ready",
    title: "Runtime ready",
};
pub(crate) const RUNTIME_LOAD_FAILED: AuditSpec = AuditSpec {
    category: "runtime",
    kind: "load_failed",
    title: "Load failed",
};
pub(crate) const RUNTIME_REUSED: AuditSpec = AuditSpec {
    category: "runtime",
    kind: "reused",
    title: "Runtime reused",
};
pub(crate) const RUNTIME_EVICT_STARTED: AuditSpec = AuditSpec {
    category: "runtime",
    kind: "evict_started",
    title: "Eviction started",
};
pub(crate) const RUNTIME_EVICT_COMPLETE: AuditSpec = AuditSpec {
    category: "runtime",
    kind: "evict_complete",
    title: "Runtime evicted",
};

pub(crate) const PROCESS_SPAWNED: AuditSpec = AuditSpec {
    category: "process",
    kind: "spawned",
    title: "Process spawned",
};
pub(crate) const PROCESS_FINISHED: AuditSpec = AuditSpec {
    category: "process",
    kind: "finished",
    title: "Process finished",
};
pub(crate) const PROCESS_ERRORED: AuditSpec = AuditSpec {
    category: "process",
    kind: "errored",
    title: "Process errored",
};
pub(crate) const PROCESS_INPUT_RECEIVED: AuditSpec = AuditSpec {
    category: "process",
    kind: "input_received",
    title: "Input received",
};
pub(crate) const PROCESS_TURN_COMPLETED: AuditSpec = AuditSpec {
    category: "process",
    kind: "output_turn_completed",
    title: "Turn completed",
};
pub(crate) const PROCESS_TERMINATED: AuditSpec = AuditSpec {
    category: "process",
    kind: "terminated",
    title: "Termination requested",
};
pub(crate) const PROCESS_KILLED: AuditSpec = AuditSpec {
    category: "process",
    kind: "killed",
    title: "Process killed",
};

pub(crate) const TOOL_DISPATCHED: AuditSpec = AuditSpec {
    category: "tool",
    kind: "dispatched",
    title: "Tool dispatched",
};
pub(crate) const TOOL_COMPLETED: AuditSpec = AuditSpec {
    category: "tool",
    kind: "completed",
    title: "Tool completed",
};
pub(crate) const TOOL_FAILED: AuditSpec = AuditSpec {
    category: "tool",
    kind: "failed",
    title: "Tool failed",
};
pub(crate) const TOOL_KILLED: AuditSpec = AuditSpec {
    category: "tool",
    kind: "killed",
    title: "Tool killed",
};

pub(crate) const ACCOUNTING_USAGE_RECORDED: AuditSpec = AuditSpec {
    category: "accounting",
    kind: "usage_recorded",
    title: "Usage recorded",
};
pub(crate) const ACCOUNTING_COST_RECORDED: AuditSpec = AuditSpec {
    category: "accounting",
    kind: "cost_recorded",
    title: "Cost recorded",
};

pub(crate) const KERNEL_BOOT_RECOVERED: AuditSpec = AuditSpec {
    category: "kernel",
    kind: "boot_recovered",
    title: "Boot recovery applied",
};

pub(crate) const KERNEL_LEGACY_RESTORE_APPLIED: AuditSpec = AuditSpec {
    category: "kernel",
    kind: "legacy_restore_applied",
    title: "Legacy restore applied",
};

#[derive(Debug, Clone, Default)]
pub(crate) struct AuditContext {
    pub(crate) session_id: Option<String>,
    pub(crate) pid: Option<u64>,
    pub(crate) runtime_id: Option<String>,
}

impl AuditContext {
    pub(crate) fn for_runtime(runtime_id: &str) -> Self {
        Self {
            session_id: None,
            pid: None,
            runtime_id: Some(runtime_id.to_string()),
        }
    }

    pub(crate) fn for_process(
        session_id: Option<&str>,
        pid: u64,
        runtime_id: Option<&str>,
    ) -> Self {
        Self {
            session_id: session_id.map(ToString::to_string),
            pid: Some(pid),
            runtime_id: runtime_id.map(ToString::to_string),
        }
    }
}

pub(crate) fn context_for_pid(
    session_registry: &SessionRegistry,
    runtime_registry: &RuntimeRegistry,
    pid: u64,
) -> AuditContext {
    AuditContext::for_process(
        session_registry.session_id_for_pid(pid),
        pid,
        runtime_registry
            .runtime_id_for_pid(pid)
            .or_else(|| session_registry.runtime_id_for_pid(pid)),
    )
}

pub(crate) fn record(
    storage: &mut StorageService,
    spec: AuditSpec,
    detail: impl AsRef<str>,
    context: AuditContext,
) {
    let record = NewAuditEvent {
        category: spec.category.to_string(),
        kind: spec.kind.to_string(),
        title: spec.title.to_string(),
        detail: compact_detail(detail.as_ref()),
        session_id: context.session_id,
        pid: context.pid,
        runtime_id: context.runtime_id,
    };

    if let Err(err) = storage.record_audit_event(&record) {
        tracing::warn!(
            category = spec.category,
            kind = spec.kind,
            %err,
            "AUDIT: failed to persist audit event"
        );
    }
}

fn compact_detail(detail: &str) -> String {
    let collapsed = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let collapsed = collapsed.trim();
    if collapsed.is_empty() {
        return "-".to_string();
    }

    const LIMIT: usize = 220;
    if collapsed.chars().count() <= LIMIT {
        return collapsed.to_string();
    }

    let mut truncated = collapsed.chars().take(LIMIT).collect::<String>();
    truncated.push_str("...");
    truncated
}
