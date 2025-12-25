mod engine;
mod memory;
mod process;
mod protocol;

use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque}; // VecDeque per il buffer FIFO
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use engine::LLMEngine;
use memory::{MemoryConfig, NeuralMemory};
use protocol::{CommandHeader, OpCode};

const SERVER: Token = Token(0);

// Enum per la macchina a stati del protocollo (come prima)
enum ClientState {
    WaitingForHeader,
    ReadingBody {
        header: CommandHeader,
        body_so_far: usize,
    },
}

struct Client {
    stream: mio::net::TcpStream,
    buffer: Vec<u8>,             // Buffer INGRESSO (TCP -> Kernel)
    output_buffer: VecDeque<u8>, // Buffer USCITA (Kernel -> TCP)
    state: ClientState,
}

impl Client {
    fn new(stream: mio::net::TcpStream) -> Self {
        Client {
            stream,
            buffer: Vec::with_capacity(4096),
            output_buffer: VecDeque::new(), // Inizialmente vuoto
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

    // Init Memory Layer (Placeholder)
    let mem_config = MemoryConfig {
        block_size: 16,
        hidden_dim: 256,
        total_memory_mb: 64,
    };
    let memory = Rc::new(RefCell::new(NeuralMemory::new(mem_config).unwrap()));

    // Init AI Engine (Shared)
    let engine_state: Arc<Mutex<Option<LLMEngine>>> = Arc::new(Mutex::new(None));

    println!(
        "Agentic OS Kernel v0.7 (Async I/O Routing) ready on {}",
        addr
    );

    loop {
        // Timeout breve per permettere allo scheduler di girare
        poll.poll(&mut events, Some(std::time::Duration::from_millis(5)))?;

        // 1. GESTIONE EVENTI RETE (I/O)
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
                    // Recupera il client
                    if let Some(client) = clients.get_mut(&token) {
                        let mut should_close = false;

                        // SE È LEGGIBILE (Il client ha mandato dati)
                        if event.is_readable() {
                            if handle_read(client, &memory, &engine_state, token.0) {
                                should_close = true;
                            }
                        }

                        // SE È SCRIVIBILE (Il socket è pronto a ricevere dati dal buffer)
                        if event.is_writable() {
                            if handle_write(client) {
                                should_close = true;
                            } else {
                                // Se abbiamo finito di scrivere, torniamo ad ascoltare solo READABLE
                                // per non svegliare la CPU inutilmente
                                if client.output_buffer.is_empty() {
                                    poll.registry().reregister(
                                        &mut client.stream,
                                        token,
                                        Interest::READABLE,
                                    )?;
                                }
                            }
                        }

                        if should_close {
                            println!("Connection closed client {}", token.0);
                            clients.remove(&token);
                        }
                    }
                }
            }
        }

        // 2. SCHEDULER (Round Robin sui processi)
        // 2. SCHEDULER
        let mut lock = engine_state.lock().unwrap();
        if let Some(engine) = lock.as_mut() {
            let active_pids = engine.list_active_pids();

            for pid in active_pids {
                match engine.step_process(pid) {
                    Ok(Some((text, owner_id))) => {
                        let token = Token(owner_id);
                        if let Some(client) = clients.get_mut(&token) {
                            // --- SYSCALL INTERCEPTION LOGIC (Semplificata) ---
                            // In un sistema reale useremmo uno state-parser più robusto.
                            // Qui controlliamo se il testo contiene i marcatori.

                            // Nota: Questo è un hack per il test.
                            // Se il token è "[[", iniziamo a bufferizzare (non inviamo al client).
                            // Se il token è "]]", eseguiamo.
                            // Per ora, assumiamo che l'agente generi il comando tutto insieme o quasi.

                            // INVIO AL CLIENT (Standard Output)
                            client.output_buffer.extend(text.as_bytes());

                            // CHECK SYSCALL (Post-processing dell'output)
                            // Controlliamo se nel buffer di uscita c'è un comando completo
                            // Questo è sporco ma efficace per demo: leggiamo l'intero buffer di uscita
                            let output_so_far =
                                String::from_utf8_lossy(client.output_buffer.make_contiguous())
                                    .to_string();
                                  
                            if let Some(start) = output_so_far.rfind("[[") {
                                if let Some(end) = output_so_far.rfind("]]") {
                                    if end > start {
                                        // ABBIAMO UNA SYSCALL!
                                        let cmd_content = &output_so_far[start + 2..end];
                                        println!(
                                            "OS: Intercepted SysCall from PID {}: {}",
                                            pid, cmd_content
                                        );

                                        // 1. Eseguiamo l'azione (Kernel Space)
                                        let result = handle_syscall(cmd_content);

                                        // 2. Iniettiamo il risultato (Context Injection)
                                        // Diciamo all'agente: "Ecco il risultato, ora continua"
                                        let _ = engine.inject_context(pid, &result);

                                        // 3. (Opzionale) Puliamo il buffer di uscita per non mostrare
                                        // i dettagli tecnici all'utente, oppure li lasciamo per debug.
                                        // Lasciamoli per ora.
                                    }
                                }
                            }

                            let _ = poll.registry().reregister(
                                &mut client.stream,
                                token,
                                Interest::READABLE | Interest::WRITABLE,
                            );
                        } else {
                            engine.kill_process(pid);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("Process {} Error: {}", pid, e);
                        engine.kill_process(pid);
                    }
                }
            }
        }
    }
}

// Gestione Lettura (TCP -> Kernel)
fn handle_read(
    client: &mut Client,
    memory: &Rc<RefCell<NeuralMemory>>,
    engine_state: &Arc<Mutex<Option<LLMEngine>>>,
    client_id: usize, // Passiamo l'ID per spawnare il processo
) -> bool {
    let mut chunk = [0; 4096];
    loop {
        match client.stream.read(&mut chunk) {
            Ok(0) => return true, // EOF
            Ok(n) => {
                client.buffer.extend_from_slice(&chunk[..n]);
                break;
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return false,
            Err(e) => {
                eprintln!("Read Err: {}", e);
                return true;
            }
        }
    }

    // Processa Buffer (Macchina a stati)
    loop {
        match &mut client.state {
            ClientState::WaitingForHeader => {
                if let Some(pos) = client.buffer.iter().position(|&b| b == b'\n') {
                    let header_bytes = client.buffer.drain(..=pos).collect::<Vec<u8>>();
                    let header_str = String::from_utf8_lossy(&header_bytes).trim().to_string();
                    if header_str.is_empty() {
                        continue;
                    }

                    match CommandHeader::parse(&header_str) {
                        Ok(header) => {
                            if header.content_length == 0 {
                                execute_command(
                                    client,
                                    header,
                                    Vec::new(),
                                    memory,
                                    engine_state,
                                    client_id,
                                );
                            } else {
                                client.state = ClientState::ReadingBody {
                                    header,
                                    body_so_far: 0,
                                };
                            }
                        }
                        Err(e) => {
                            // Rispondi errore (nel buffer di uscita)
                            let msg = protocol::response_err(&e);
                            client.output_buffer.extend(msg);
                        }
                    }
                } else {
                    break;
                }
            }
            ClientState::ReadingBody {
                header,
                body_so_far: _,
            } => {
                if client.buffer.len() >= header.content_length {
                    let payload = client
                        .buffer
                        .drain(..header.content_length)
                        .collect::<Vec<u8>>();
                    // Ricostruiamo header per passarlo (clone necessario)
                    let h = CommandHeader {
                        opcode: header.opcode.clone(),
                        agent_id: header.agent_id.clone(),
                        content_length: header.content_length,
                    };

                    execute_command(client, h, payload, memory, engine_state, client_id);
                    client.state = ClientState::WaitingForHeader;
                } else {
                    break;
                }
            }
        }
    }
    false
}

// Gestione Scrittura (Kernel -> TCP)
fn handle_write(client: &mut Client) -> bool {
    // Finché c'è roba nel buffer, prova a inviarla
    while !client.output_buffer.is_empty() {
        // Prendiamo la prima fetta contigua del buffer circolare
        let (head, _) = client.output_buffer.as_slices();
        match client.stream.write(head) {
            Ok(n) => {
                // Rimuoviamo i byte scritti
                client.output_buffer.drain(..n);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return false, // Buffer OS pieno, riprova dopo
            Err(e) => {
                eprintln!("Write Err: {}", e);
                return true;
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
    client_id: usize, // <--- ID Proprietario
) {
    let response = match header.opcode {
        OpCode::Ping => protocol::response_ok("PONG"),
        OpCode::Load => {
            let path = String::from_utf8_lossy(&payload).trim().to_string();
            // Aggiungi un feedback utente
            println!("CMD: Loading MASTER MODEL (Zero-Copy Base) from {}", path);
            match LLMEngine::load(&path) {
                Ok(new_engine) => {
                    let mut lock = engine_state.lock().unwrap();
                    *lock = Some(new_engine);
                    protocol::response_ok("Master Model Loaded. Ready to fork agents.")
                }
                Err(e) => protocol::response_err(&format!("Load Failed: {}", e)),
            }
        }
        OpCode::Exec => {
            let prompt = String::from_utf8_lossy(&payload).to_string();
            let mut lock = engine_state.lock().unwrap();

            if let Some(engine) = lock.as_mut() {
                // Passiamo client_id allo spawn
                match engine.spawn_process(&prompt, 150, client_id) {
                    Ok(pid) => protocol::response_ok(&format!("Process Started PID: {}", pid)),
                    Err(e) => protocol::response_err(&format!("Spawn Failed: {}", e)),
                }
            } else {
                protocol::response_err("No Model Loaded")
            }
        }
        _ => protocol::response_err("Not Implemented"),
    };

    // Scrivi risposta nel buffer di uscita
    client.output_buffer.extend(response);
}

// Funzione helper per eseguire "System Calls" simulate
fn handle_syscall(command: &str) -> String {
    // Formato atteso: "CALC: 2+2"
    if command.starts_with("CALC:") {
        let expr = command.trim_start_matches("CALC:").trim();
        // Per ora facciamo una eval molto stupida o usiamo un crate,
        // ma per testare il loop basta simulare:
        if expr.contains('+') {
            let parts: Vec<&str> = expr.split('+').collect();
            if parts.len() == 2 {
                let a: i32 = parts[0].trim().parse().unwrap_or(0);
                let b: i32 = parts[1].trim().parse().unwrap_or(0);
                return format!("SysCall Result: {}\n", a + b);
            }
        }
        return "SysCall Error: Invalid Math\n".to_string();
    }
    "SysCall Error: Unknown Command\n".to_string()
}
