mod client;
mod framing;
mod io;

pub use client::{Client, ClientState, ParsedCommand};
pub use framing::parse_available_commands;
#[cfg(test)]
pub use io::handle_read;
#[cfg(test)]
pub use io::handle_read_with_test_state;
pub use io::{handle_read_with_registry, handle_write};

use mio::Interest;

pub fn needs_writable_interest(client: &Client) -> bool {
    !client.output_buffer.is_empty()
}

pub fn writable_interest() -> Interest {
    Interest::READABLE | Interest::WRITABLE
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
