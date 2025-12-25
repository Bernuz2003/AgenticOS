mod protocol;

use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use protocol::CommandHeader;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::rc::Rc;
mod memory;
use memory::{MemoryConfig, NeuralMemory};
mod engine;
use engine::LLMEngine;
use std::sync::{Arc, Mutex};

const SERVER: Token = Token(0);

// --- LA MACCHINA A STATI ---
enum ClientState {
    WaitingForHeader,
    ReadingBody {
        header: CommandHeader,
        body_so_far: usize,
    },
}

struct Client {
    stream: TcpStream,
    buffer: Vec<u8>,    // Accumulatore di dati grezzi
    state: ClientState, // Cosa stiamo aspettando?
}

impl Client {
    fn new(stream: TcpStream) -> Self {
        Client {
            stream,
            buffer: Vec::with_capacity(4096),
            state: ClientState::WaitingForHeader,
        }
    }
}

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
        total_memory_mb: 64, // 64MB bastano per il test
    };

    // Gestione errore inizializzazione Candle
    let neural_mem = NeuralMemory::new(mem_config).expect("Failed to initialize Candle/GPU Memory");

    let memory = Rc::new(RefCell::new(neural_mem));

    // Lo stato globale dell'Engine IA. Inizialmente vuoto (None).
    // Usiamo Arc<Mutex> per poterlo condividere (e in futuro passare a thread worker)
    let engine_state: Arc<Mutex<Option<LLMEngine>>> = Arc::new(Mutex::new(None));

    println!("Agentic OS Kernel v0.5 (AI Inference) ready...");

    loop {
        poll.poll(&mut events, None)?;

        for event in events.iter() {
            match event.token() {
                SERVER => loop {
                    match server.accept() {
                        Ok((mut stream, addr)) => {
                            let token = unique_token;
                            unique_token.0 += 1;
                            println!("New connection: {}", addr);
                            poll.registry()
                                .register(&mut stream, token, Interest::READABLE)?;
                            clients.insert(token, Client::new(stream));
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => eprintln!("Accept error: {}", e),
                    }
                },
                token => {
                    let should_close = if let Some(client) = clients.get_mut(&token) {
                        handle_client_io(client, &memory, &engine_state)
                    } else {
                        false
                    };

                    if should_close {
                        println!("Connection closed.");
                        clients.remove(&token);
                    }
                }
            }
        }
    }
}

fn handle_client_io(
    client: &mut Client,
    memory: &Rc<RefCell<NeuralMemory>>,
    engine: &Arc<Mutex<Option<LLMEngine>>>,
) -> bool {
    let mut chunk = [0; 4096];

    // 1. Leggi dal socket e appendi al buffer
    loop {
        match client.stream.read(&mut chunk) {
            Ok(0) => return true, // EOF
            Ok(n) => {
                client.buffer.extend_from_slice(&chunk[..n]);
                break;
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return false, // Niente dati, torna al poll
            Err(e) => {
                eprintln!("IO Error: {}", e);
                return true;
            }
        }
    }

    // 2. Processa il buffer in base allo stato
    loop {
        match &mut client.state {
            ClientState::WaitingForHeader => {
                // Cerchiamo il carattere newline '\n'
                if let Some(pos) = client.buffer.iter().position(|&b| b == b'\n') {
                    // Estraiamo la linea di header
                    let header_line_bytes = client.buffer.drain(..=pos).collect::<Vec<u8>>();
                    let header_str = String::from_utf8_lossy(&header_line_bytes)
                        .trim()
                        .to_string();

                    if header_str.is_empty() {
                        continue;
                    } // Ignora righe vuote

                    println!("DEBUG Header: '{}'", header_str);

                    match CommandHeader::parse(&header_str) {
                        Ok(header) => {
                            if header.content_length == 0 {
                                // Comando senza payload (es. PING), esegui subito
                                execute_command(client, header, Vec::new(), memory, engine);
                                // Rimaniamo in WaitingForHeader
                            } else {
                                // Passiamo a leggere il corpo
                                println!("DEBUG Expecting body: {} bytes", header.content_length);
                                client.state = ClientState::ReadingBody {
                                    header,
                                    body_so_far: 0,
                                };
                            }
                        }
                        Err(e) => {
                            let _ = client
                                .stream
                                .write_all(protocol::response_err(&e).as_slice());
                        }
                    }
                } else {
                    // Header incompleto, aspettiamo altri pacchetti TCP
                    break;
                }
            }

            ClientState::ReadingBody {
                header,
                body_so_far: _,
            } => {
                // Abbiamo abbastanza dati nel buffer per soddisfare la richiesta?
                if client.buffer.len() >= header.content_length {
                    // Sì! Estraiamo il payload esatto
                    let payload = client
                        .buffer
                        .drain(..header.content_length)
                        .collect::<Vec<u8>>();

                    // Clona l'header per passarlo all'esecuzione (perché `header` qui è un borrow mutabile)
                    let h = CommandHeader {
                        opcode: header.opcode.clone(),
                        agent_id: header.agent_id.clone(),
                        content_length: header.content_length,
                    };

                    // Esegui
                    execute_command(client, h, payload, memory, engine);

                    // Reset stato
                    client.state = ClientState::WaitingForHeader;
                } else {
                    // Non ancora, aspettiamo altri dati
                    break;
                }
            }
        }
    }

    false
}

fn execute_command(
    client: &mut Client,
    header: protocol::CommandHeader,
    payload: Vec<u8>,
    _memory: &Rc<RefCell<NeuralMemory>>,
    engine_state: &Arc<Mutex<Option<LLMEngine>>>,
) {
    use protocol::OpCode;

    let response = match header.opcode {
        OpCode::Ping => protocol::response_ok("PONG"),

        // LOAD: Carica il modello GGUF specificato nel payload
        OpCode::Load => {
            // Interpretiamo il payload come path del file
            let path = String::from_utf8_lossy(&payload).trim().to_string();

            // Rispondiamo subito che stiamo caricando (operazione bloccante per ora)
            println!("CMD: Loading Model from {}", path);

            match LLMEngine::load(&path) {
                Ok(new_engine) => {
                    let mut lock = engine_state.lock().unwrap();
                    *lock = Some(new_engine);
                    protocol::response_ok("Model Loaded Successfully")
                }
                Err(e) => protocol::response_err(&format!("Load Failed: {}", e)),
            }
        }

        // EXEC: Esegue inferenza vera
        OpCode::Exec => {
            let prompt = String::from_utf8_lossy(&payload).to_string();
            let mut lock = engine_state.lock().unwrap();

            if let Some(engine) = lock.as_mut() {
                // Generiamo max 100 token per test
                match engine.predict(&prompt, 100) {
                    Ok(text) => protocol::response_ok(&text),
                    Err(e) => protocol::response_err(&format!("Inference Error: {}", e)),
                }
            } else {
                protocol::response_err("No Model Loaded. Use LOAD <path> first.")
            }
        }

        // Manteniamo i comandi di memoria per debug
        OpCode::MemoryWrite => {
            // ... logica vecchia ...
            protocol::response_ok("Memory Ops still supported")
        }

        _ => protocol::response_err("Not Implemented"),
    };

    let _ = client.stream.write_all(&response);
}
