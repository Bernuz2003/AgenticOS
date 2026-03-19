pub mod auth;
pub mod client;
pub mod composer;
pub mod error;
pub mod events;
pub mod history_db;
pub mod protocol;
pub mod stream;

// Role aliases used by bridge code to make source-of-truth intent explicit.
// - persisted_truth: SQLite-backed historical truth.
// - live_cache: in-memory live view derived from kernel event stream.
pub use history_db as persisted_truth;
pub use stream as live_cache;
