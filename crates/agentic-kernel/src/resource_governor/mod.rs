//! Resource Governor and Admission Control
//! 
//! The Resource Governor acts as the gatekeeper for VRAM and RAM allocations across the
//! kernel. It prevents Out-Of-Memory (OOM) crashes by mathematically proving that a 
//! model activation fits within the configured budget *before* it is spawned.
//! 
//! **Design Principles:**
//! - **Pessimistic Allocation**: We account for worst-case memory overheads (e.g. KV cache 
//!   at `max_tokens`, fixed framework allocations).
//! - **Queueing**: If a requested activation exceeds available headroom, the request is 
//!   queued rather than rejected, allowing the system to backpressure gracefully.

//! Global admission control and VRAM/RAM budgeting for runtimes.

mod state;
mod admission;
mod governor;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub(crate) use state::*;
pub(crate) use governor::*;
