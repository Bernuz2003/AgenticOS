mod core;
mod residency;
pub(crate) mod swap;
mod swap_io;
mod types;

pub use core::NeuralMemory;

#[allow(unused_imports)]
pub use types::{ContextSlotId, MemorySnapshot, SlotPersistenceKind, SwapEvent};
