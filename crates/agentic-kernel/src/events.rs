use std::collections::HashMap;

use agentic_control_models::{KernelEvent, KernelEventEnvelope};
use mio::{Poll, Token};

use crate::protocol;
use crate::storage::StorageService;
use crate::transport::{writable_interest, Client};

pub fn flush_pending_events(
    clients: &mut HashMap<Token, Client>,
    poll: &Poll,
    next_sequence: &mut u64,
    storage: &mut StorageService,
    pending_events: &mut Vec<KernelEvent>,
) {
    if pending_events.is_empty() {
        return;
    }

    for event in pending_events.drain(..) {
        if let Err(err) = storage.record_kernel_event(&event) {
            tracing::error!(%err, "EVENTS: failed to persist kernel event");
        }
        *next_sequence = next_sequence.saturating_add(1);
        let envelope = KernelEventEnvelope {
            seq: *next_sequence,
            event,
        };
        let payload = match serde_json::to_vec(&envelope) {
            Ok(payload) => payload,
            Err(err) => {
                tracing::error!(%err, "EVENTS: failed to serialize kernel event");
                continue;
            }
        };
        let frame = protocol::response_data_with_code("event", &payload);

        for (token, client) in clients.iter_mut() {
            if !client.subscribed_events {
                continue;
            }

            client.output_buffer.extend(frame.iter().copied());
            let _ = poll
                .registry()
                .reregister(&mut client.stream, *token, writable_interest());
        }
    }
}
