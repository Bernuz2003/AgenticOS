mod core;
mod eviction;
mod swap_io;
mod types;

pub use core::NeuralMemory;
pub use types::MemoryConfig;

// Disponibili per uso futuro a livello crate
#[allow(unused_imports)]
pub use types::{MemorySnapshot, SwapEvent, TensorId};
