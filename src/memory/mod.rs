mod core;
mod eviction;
pub(crate) mod swap;
mod swap_io;
mod types;

pub use core::NeuralMemory;
pub use types::MemoryConfig;

// Disponibili per uso futuro a livello crate
#[allow(unused_imports)]
pub use types::{ContextSlotId, MemorySnapshot, SwapEvent, TensorId};
