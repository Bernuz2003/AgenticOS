/// Process state tracking and policies.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessState {
    Ready,
    Running,
    AwaitingTurnDecision,
    WaitingForInput,
    WaitingForHumanInput,
    Parked,
    WaitingForSyscall,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResidentSlotState {
    #[default]
    Unbound,
    Allocated,
    ParkRequested,
    SnapshotSaved,
    Restoring,
}

impl ResidentSlotState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unbound => "unbound",
            Self::Allocated => "allocated",
            Self::ParkRequested => "park_requested",
            Self::SnapshotSaved => "snapshot_saved",
            Self::Restoring => "restoring",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResidentSlotPolicy {
    #[default]
    Unmanaged,
    ParkAndResume,
}

impl ResidentSlotPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unmanaged => "unmanaged",
            Self::ParkAndResume => "park_and_resume",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessLifecyclePolicy {
    Ephemeral,
    Interactive,
}

impl ProcessLifecyclePolicy {
    pub fn is_interactive(self) -> bool {
        matches!(self, Self::Interactive)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HumanInputRequestKind {
    #[default]
    Question,
    Approval,
}

impl HumanInputRequestKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::Approval => "approval",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct HumanInputRequest {
    pub request_id: String,
    pub kind: HumanInputRequestKind,
    pub question: String,
    pub details: Option<String>,
    pub choices: Vec<String>,
    pub allow_free_text: bool,
    pub placeholder: Option<String>,
    pub requested_at_ms: i64,
}
