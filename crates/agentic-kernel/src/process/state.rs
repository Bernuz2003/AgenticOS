/// Process state tracking and policies.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessState {
    Ready,
    Running,
    AwaitingTurnDecision,
    WaitingForInput,
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
