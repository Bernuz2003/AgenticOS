use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::commands::execute_command;
use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::prompting::PromptFamily;
use crate::protocol;

use super::{parse_available_commands, Client, ParsedCommand};

pub fn handle_read(
    client: &mut Client,
    memory: &Rc<RefCell<NeuralMemory>>,
    engine_state: &Arc<Mutex<Option<LLMEngine>>>,
    model_catalog: &mut ModelCatalog,
    active_family: &mut PromptFamily,
    client_id: usize,
    shutdown_requested: &Arc<AtomicBool>,
) -> bool {
    let mut chunk = [0; 4096];
    loop {
        match client.stream.read(&mut chunk) {
            Ok(0) => return true,
            Ok(n) => {
                client.buffer.extend_from_slice(&chunk[..n]);
                break;
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return false,
            Err(ref e)
                if e.kind() == io::ErrorKind::ConnectionReset
                    || e.kind() == io::ErrorKind::BrokenPipe =>
            {
                return true;
            }
            Err(e) => {
                eprintln!("Read error: {}", e);
                return true;
            }
        }
    }

    let parsed = parse_available_commands(&mut client.buffer, &mut client.state);
    for command in parsed {
        match command {
            ParsedCommand::Ok { header, payload } => execute_command(
                client,
                header,
                payload,
                memory,
                engine_state,
                model_catalog,
                active_family,
                client_id,
                shutdown_requested,
            ),
            ParsedCommand::Err(e) => {
                client
                    .output_buffer
                    .extend(protocol::response_err_code("BAD_HEADER", &e));
            }
        }
    }
    false
}

pub fn handle_write(client: &mut Client) -> bool {
    while !client.output_buffer.is_empty() {
        let (head, _) = client.output_buffer.as_slices();
        match client.stream.write(head) {
            Ok(n) => {
                client.output_buffer.drain(..n);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return false,
            Err(_) => return true,
        }
    }
    false
}
