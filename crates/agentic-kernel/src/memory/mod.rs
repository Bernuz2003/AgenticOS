mod core;
mod residency;
mod restore;
pub(crate) mod swap;
mod types;

pub use core::NeuralMemory;

#[allow(unused_imports)]
pub use types::{ContextSlotId, MemorySnapshot, SlotPersistenceKind, SwapEvent};
