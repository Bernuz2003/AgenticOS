use agentic_control_models::AssistantSegmentKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum InFlightAssistantPhase {
    StreamingVisible,
    StreamingThinking,
    InvocationPending,
    AwaitingBoundaryCommit,
    #[default]
    Closed,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct InFlightAssistantTurn {
    pub(super) raw_transport_text: String,
    pub(super) visible_projection: String,
    pub(super) thinking_projection: String,
    pub(super) reasoning_sidecar_projection: String,
    pub(super) pending_visible_buffer: String,
    pub(super) pending_thinking_buffer: String,
    pub(super) pending_invocation: Option<String>,
    pub(super) in_thinking_block: bool,
    pub(super) generated_token_count: usize,
    pub(super) phase: InFlightAssistantPhase,
}

impl InFlightAssistantTurn {
    pub(crate) fn raw_transport_text(&self) -> &str {
        &self.raw_transport_text
    }

    pub(crate) fn visible_projection(&self) -> &str {
        &self.visible_projection
    }

    pub(crate) fn thinking_projection(&self) -> &str {
        &self.thinking_projection
    }

    pub(crate) fn pending_invocation(&self) -> Option<&str> {
        self.pending_invocation.as_deref()
    }

    pub(crate) fn clear_runtime_state(&mut self) {
        self.raw_transport_text.clear();
        self.visible_projection.clear();
        self.thinking_projection.clear();
        self.reasoning_sidecar_projection.clear();
        self.pending_visible_buffer.clear();
        self.pending_thinking_buffer.clear();
        self.pending_invocation = None;
        self.in_thinking_block = false;
        self.generated_token_count = 0;
        self.phase = InFlightAssistantPhase::Closed;
    }

    pub(crate) fn accumulate_generated_tokens(&mut self, token_count: usize) {
        self.generated_token_count = self.generated_token_count.saturating_add(token_count);
    }

    pub(crate) fn generated_token_count(&self) -> usize {
        self.generated_token_count
    }

    pub(crate) fn mark_boundary_commit_pending(&mut self) {
        self.phase = InFlightAssistantPhase::AwaitingBoundaryCommit;
    }

    pub(super) fn recompute_phase(&mut self) {
        self.phase = if self.pending_invocation.is_some() {
            InFlightAssistantPhase::InvocationPending
        } else if self.in_thinking_block {
            InFlightAssistantPhase::StreamingThinking
        } else if self.raw_transport_text.is_empty()
            && self.visible_projection.is_empty()
            && self.thinking_projection.is_empty()
            && self.reasoning_sidecar_projection.is_empty()
        {
            InFlightAssistantPhase::Closed
        } else {
            InFlightAssistantPhase::StreamingVisible
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AssistantOutputSegment {
    pub(crate) kind: AssistantSegmentKind,
    pub(crate) text: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct AssistantOutputFragment {
    pub(crate) segments: Vec<AssistantOutputSegment>,
    pub(crate) syscall_command: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThinkingSource {
    Inline,
    Sidecar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AssistantSemanticEvent {
    RawTransportText(String),
    VisibleText(String),
    ThinkingText {
        text: String,
        source: ThinkingSource,
    },
    InvocationDetected(String),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct AssistantTurnDelta {
    pub(super) events: Vec<AssistantSemanticEvent>,
}

impl AssistantTurnDelta {
    pub(super) fn push(&mut self, event: AssistantSemanticEvent) {
        self.events.push(event);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AssistantTransportEvent {
    pub(super) kind: AssistantSegmentKind,
    pub(super) text: String,
}

impl AssistantTransportEvent {
    pub(super) fn from_fragment(kind: AssistantSegmentKind, fragment: &str) -> Option<Self> {
        if fragment.is_empty() {
            return None;
        }

        Some(Self {
            kind,
            text: fragment.to_string(),
        })
    }
}
