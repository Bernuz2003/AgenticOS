mod backend;
mod commands;
mod engine;
mod memory;
mod model_catalog;
mod process;
mod prompting;
mod protocol;
mod runtime;
mod tools;
mod transport;

use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use engine::LLMEngine;
use memory::{MemoryConfig, NeuralMemory};
use model_catalog::ModelCatalog;
use prompting::PromptFamily;
use runtime::run_engine_tick;
use transport::{handle_read, handle_write, needs_writable_interest, writable_interest, Client};

const SERVER: Token = Token(0);

fn main() -> io::Result<()> {
    env_logger::init();
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);
    let addr = "127.0.0.1:6379".parse().unwrap();
    let mut server = TcpListener::bind(addr)?;
    poll.registry()
        .register(&mut server, SERVER, Interest::READABLE)?;

    let mut clients: HashMap<Token, Client> = HashMap::new();
    let mut unique_token = Token(SERVER.0 + 1);

    let mem_config = MemoryConfig {
        block_size: 16,
        hidden_dim: 256,
        total_memory_mb: 64,
    };
    let memory = Rc::new(RefCell::new(NeuralMemory::new(mem_config).unwrap()));
    let engine_state: Arc<Mutex<Option<LLMEngine>>> = Arc::new(Mutex::new(None));
    let shutdown_requested: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let mut model_catalog =
        ModelCatalog::discover("models").map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let mut active_family: PromptFamily = PromptFamily::Llama;

    println!(
        "Agentic OS Kernel v1.3 (Process-Centric SysCalls) ready on {}",
        addr
    );

    loop {
        if shutdown_requested.load(Ordering::SeqCst) {
            println!("Kernel graceful shutdown requested. Closing event loop.");
            break;
        }

        poll.poll(&mut events, Some(std::time::Duration::from_millis(5)))?;

        for event in events.iter() {
            match event.token() {
                SERVER => loop {
                    match server.accept() {
                        Ok((mut stream, peer_addr)) => {
                            let token = unique_token;
                            unique_token.0 += 1;
                            println!("New connection: {}", peer_addr);
                            poll.registry()
                                .register(&mut stream, token, Interest::READABLE)?;
                            clients.insert(token, Client::new(stream));
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => eprintln!("Accept error: {}", e),
                    }
                },
                token => {
                    if let Some(client) = clients.get_mut(&token) {
                        let mut should_close = false;

                        if event.is_readable()
                            && handle_read(
                                client,
                                &memory,
                                &engine_state,
                                &mut model_catalog,
                                &mut active_family,
                                token.0,
                                &shutdown_requested,
                            )
                        {
                            should_close = true;
                        }

                        if !should_close && needs_writable_interest(client) {
                            poll.registry()
                                .reregister(&mut client.stream, token, writable_interest())?;
                        }

                        if event.is_writable() {
                            if handle_write(client) {
                                should_close = true;
                            } else if client.output_buffer.is_empty() {
                                poll.registry().reregister(
                                    &mut client.stream,
                                    token,
                                    Interest::READABLE,
                                )?;
                            }
                        }

                        if should_close {
                            clients.remove(&token);
                        }
                    }
                }
            }
        }

        run_engine_tick(&engine_state, &mut clients, &poll, active_family);
    }

    Ok(())
}
