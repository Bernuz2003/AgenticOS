use agentic_control_models::{CoreDumpRequest, CoreDumpSummaryView, KernelEvent};

use crate::config::kernel_config;

use super::capture::{capture_core_dump, CaptureCoreDumpArgs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomaticCaptureKind {
    Kill,
    Error,
}

pub(crate) fn maybe_capture_automatic_core_dump(
    args: CaptureCoreDumpArgs<'_>,
    pid: u64,
    reason: impl Into<String>,
    note: Option<String>,
    kind: AutomaticCaptureKind,
) -> Result<Option<CoreDumpSummaryView>, String> {
    let config = &kernel_config().core_dump;
    if !config.enabled {
        return Ok(None);
    }

    let enabled = match kind {
        AutomaticCaptureKind::Kill => config.auto_capture_on_kill,
        AutomaticCaptureKind::Error => config.auto_capture_on_error,
    };
    if !enabled {
        return Ok(None);
    }

    capture_core_dump(
        args,
        CoreDumpRequest {
            pid: Some(pid),
            session_id: None,
            mode: Some("automatic".to_string()),
            reason: Some(reason.into()),
            include_workspace: Some(config.include_workspace_by_default),
            include_backend_state: Some(config.include_backend_state_by_default),
            freeze_target: Some(false),
            note,
        },
    )
    .map(Some)
}

pub(crate) fn core_dump_created_event(summary: &CoreDumpSummaryView) -> Option<KernelEvent> {
    Some(KernelEvent::CoreDumpCreated {
        pid: summary.pid?,
        session_id: summary.session_id.clone(),
        dump_id: summary.dump_id.clone(),
        reason: summary.reason.clone(),
        fidelity: summary.fidelity.clone(),
    })
}

pub(crate) fn compact_note(note: &str) -> Option<String> {
    let trimmed = note.trim();
    if trimmed.is_empty() {
        return None;
    }
    const MAX_CHARS: usize = 512;
    let mut out = String::new();
    for ch in trimmed.chars().take(MAX_CHARS) {
        out.push(ch);
    }
    if trimmed.chars().count() > MAX_CHARS {
        out.push_str("...");
    }
    Some(out)
}
