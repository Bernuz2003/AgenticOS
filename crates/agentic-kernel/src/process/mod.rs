//! Process Management and Context State
//!
//! AgenticOS separates the concept of a `Session` from a `PID` (Process ID).
//!
//! - **Session**: A persistent, durable conversation or background task. A Session retains
//!   history, context summaries, and long-term memory across system reboots. It is backed
//!   by SQLite and a persistent Context Slot on disk.
//! - **PID (Process ID)**: An ephemeral, runtime execution instance of a Session. When a
//!   Session becomes active, it is assigned a PID and loaded into RAM/VRAM. If the system
//!   crashes or the Session is parked, the PID is lost, but the Session endures.

//! Agent process management and context logic.

mod agent;
mod context;
mod state;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub use agent::*;
pub use context::*;
pub use state::*;
