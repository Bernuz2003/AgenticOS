mod backend;
mod config;
mod commands;
mod engine;
mod errors;
mod memory;
mod model_catalog;
mod process;
mod prompting;
mod protocol;
mod runtime;
mod scheduler;
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

use config::env_bool;
use engine::LLMEngine;
use memory::{MemoryConfig, NeuralMemory};
use model_catalog::ModelCatalog;
use prompting::PromptFamily;
use runtime::run_engine_tick;
use scheduler::ProcessScheduler;
use transport::{handle_read, handle_write, needs_writable_interest, writable_interest, Client};

const SERVER: Token = Token(0);

/// Encapsulates all kernel state into a single structure.
struct Kernel {
    poll: Poll,
    events: Events,
    server: TcpListener,
    clients: HashMap<Token, Client>,
    unique_token: Token,
    log_connections: bool,
    memory: Rc<RefCell<NeuralMemory>>,
    engine_state: Arc<Mutex<Option<LLMEngine>>>,
    shutdown_requested: Arc<AtomicBool>,
    model_catalog: ModelCatalog,
    active_family: PromptFamily,
    scheduler: ProcessScheduler,
}

impl Kernel {
    fn new() -> io::Result<Self> {
        let poll = Poll::new()?;
        let events = Events::with_capacity(128);
        let addr = "127.0.0.1:6379".parse().unwrap();
        let mut server = TcpListener::bind(addr)?;
        poll.registry()
            .register(&mut server, SERVER, Interest::READABLE)?;

        let log_connections = env_bool("AGENTIC_LOG_CONNECTIONS", false);

        let mem_config = MemoryConfig {
            block_size: 16,
            hidden_dim: 256,
            total_memory_mb: 64,
        };
        let memory = Rc::new(RefCell::new(
            NeuralMemory::new(mem_config)
                .map_err(|e| io::Error::other(e.to_string()))?,
        ));
        let memory_active = env_bool("AGENTIC_MEMORY_ACTIVE", true);
        let memory_swap_async = env_bool("AGENTIC_MEMORY_SWAP_ASYNC", true);
        let memory_swap_dir = std::env::var("AGENTIC_MEMORY_SWAP_DIR").ok();
        memory.borrow_mut().set_active(memory_active);
        if let Err(e) = memory.borrow_mut().configure_async_swap(
            memory_swap_async,
            memory_swap_dir
                .as_ref()
                .map(|v| std::path::PathBuf::from(v.as_str())),
        ) {
            tracing::error!(%e, "Failed to configure async swap worker");
        }

        let engine_state: Arc<Mutex<Option<LLMEngine>>> = Arc::new(Mutex::new(None));
        let shutdown_requested: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
        let model_catalog = ModelCatalog::discover("models")
            .map_err(io::Error::other)?;
        let active_family: PromptFamily = PromptFamily::Llama;
        let scheduler = ProcessScheduler::new();

        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            %addr,
            memory_active,
            memory_swap_async,
            swap_dir = memory_swap_dir.as_deref().unwrap_or("workspace/swap"),
            "AgenticOS Kernel ready"
        );

        Ok(Kernel {
            poll,
            events,
            server,
            clients: HashMap::new(),
            unique_token: Token(SERVER.0 + 1),
            log_connections,
            memory,
            engine_state,
            shutdown_requested,
            model_catalog,
            active_family,
            scheduler,
        })
    }

    fn run(&mut self) -> io::Result<()> {
        loop {
            if self.shutdown_requested.load(Ordering::SeqCst) {
                tracing::info!("Kernel graceful shutdown requested. Closing event loop.");
                break;
            }

            self.poll
                .poll(&mut self.events, Some(std::time::Duration::from_millis(5)))?;

            for event in self.events.iter() {
                match event.token() {
                    SERVER => loop {
                        match self.server.accept() {
                            Ok((mut stream, peer_addr)) => {
                                let token = self.unique_token;
                                self.unique_token.0 += 1;
                                if self.log_connections {
                                    tracing::info!(%peer_addr, "New connection");
                                }
                                self.poll.registry().register(
                                    &mut stream,
                                    token,
                                    Interest::READABLE,
                                )?;
                                self.clients.insert(token, Client::new(stream));
                            }
                            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                            Err(e) => tracing::error!(%e, "Accept error"),
                        }
                    },
                    token => {
                        if let Some(client) = self.clients.get_mut(&token) {
                            let mut should_close = false;

                            if event.is_readable()
                                && handle_read(
                                    client,
                                    &self.memory,
                                    &self.engine_state,
                                    &mut self.model_catalog,
                                    &mut self.active_family,
                                    &mut self.scheduler,
                                    token.0,
                                    &self.shutdown_requested,
                                )
                            {
                                should_close = true;
                            }

                            if !should_close && needs_writable_interest(client) {
                                self.poll.registry().reregister(
                                    &mut client.stream,
                                    token,
                                    writable_interest(),
                                )?;
                            }

                            if event.is_writable() {
                                if handle_write(client) {
                                    should_close = true;
                                } else if client.output_buffer.is_empty() {
                                    self.poll.registry().reregister(
                                        &mut client.stream,
                                        token,
                                        Interest::READABLE,
                                    )?;
                                }
                            }

                            if should_close {
                                self.clients.remove(&token);
                            }
                        }
                    }
                }
            }

            run_engine_tick(
                &self.engine_state,
                &self.memory,
                &mut self.clients,
                &self.poll,
                self.active_family,
                &mut self.scheduler,
            );
        }

        Ok(())
    }
}

fn main() -> io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut kernel = Kernel::new()?;
    kernel.run()
}
