mod engine;
mod memory;
mod process;
mod protocol;

use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use engine::LLMEngine;
use memory::{MemoryConfig, NeuralMemory};
use protocol::{CommandHeader, OpCode};

const SERVER: Token = Token(0);
const WORKSPACE_DIR: &str = "./workspace";

enum ClientState {
    WaitingForHeader,
    ReadingBody {
        header: CommandHeader,
        body_so_far: usize,
    },
}

struct Client {
    stream: mio::net::TcpStream,
    buffer: Vec<u8>,
    output_buffer: VecDeque<u8>,
    syscall_buffer: String,
    state: ClientState,
}

impl Client {
    fn new(stream: mio::net::TcpStream) -> Self {
        Client {
            stream,
            buffer: Vec::with_capacity(4096),
            output_buffer: VecDeque::new(),
            syscall_buffer: String::new(),
            state: ClientState::WaitingForHeader,
        }
    }
}

fn run_python_code(code: &str) -> String {
    // Pulizia del codice
    let clean_code = code
        .trim()
        .trim_start_matches("```python")
        .trim_start_matches("```")
        .trim_end_matches("```");

    println!("OS: Executing Python Code:\n---\n{}\n---", clean_code);

    // Scriviamo il codice in un file temporaneo per gestire script multilinea complessi
    let temp_filename = "agent_script_temp.py";
    if let Err(e) = std::fs::write(temp_filename, clean_code) {
        return format!("SysCall Error: Failed to write temp file: {}", e);
    }

    // Eseguiamo python3 sul file
    let output = Command::new("python3").arg(temp_filename).output();

    // Gestione Risultato
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);

            // Uniamo stdout e stderr per dare feedback completo all'agente
            let result = if !stderr.is_empty() {
                format!("Output:\n{}\nErrors:\n{}", stdout, stderr)
            } else {
                format!("{}", stdout) // Solo output pulito se non ci sono errori
            };

            // Pulizia file temp
            let _ = std::fs::remove_file(temp_filename);

            // Limitiamo la lunghezza per non intasare il contesto dell'agente
            let max_len = 2000;
            if result.len() > max_len {
                format!("{}... (Output Truncated)", &result[..max_len])
            } else if result.trim().is_empty() {
                "Done (No Output)".to_string()
            } else {
                result.to_string()
            }
        }
        Err(e) => format!("SysCall Error: Failed to execute python3: {}", e),
    }
}

/// Verifica che il path sia sicuro e dentro la cartella workspace
fn resolve_safe_path(filename: &str) -> Option<PathBuf> {
    let clean_name = filename.trim();

    // Sicurezza basilare: rifiuta path con ".." o "/" o "\" all'inizio
    if clean_name.contains("..") || clean_name.starts_with('/') || clean_name.starts_with('\\') {
        return None;
    }

    let mut path = PathBuf::from(WORKSPACE_DIR);
    path.push(clean_name);
    Some(path)
}

fn handle_write_file(args: &str) -> String {
    // Formato atteso: "nomefile.ext | contenuto..."
    // Usiamo '|' come separatore perché è raro nei nomi file
    let parts: Vec<&str> = args.splitn(2, '|').collect();

    if parts.len() < 2 {
        return "SysCall Error: Usage [[WRITE_FILE: filename | content]]".to_string();
    }

    let filename = parts[0].trim();
    let content = parts[1].trim_start(); // Manteniamo l'indentazione del contenuto

    // 1. Assicuriamoci che il workspace esista
    if let Err(e) = fs::create_dir_all(WORKSPACE_DIR) {
        return format!("SysCall Error: Failed to create workspace: {}", e);
    }

    // 2. Risolvi path sicuro
    if let Some(path) = resolve_safe_path(filename) {
        println!("OS: Writing file {:?}", path);
        match fs::write(&path, content) {
            Ok(_) => format!(
                "Success: File '{}' written ({} bytes).",
                filename,
                content.len()
            ),
            Err(e) => format!("SysCall Error: Write failed: {}", e),
        }
    } else {
        "SysCall Error: Invalid filename or security violation.".to_string()
    }
}

fn handle_read_file(filename: &str) -> String {
    if let Some(path) = resolve_safe_path(filename) {
        println!("OS: Reading file {:?}", path);
        match fs::read_to_string(&path) {
            Ok(content) => content, // Ritorna il contenuto grezzo
            Err(e) => format!("SysCall Error: Read failed: {}", e),
        }
    } else {
        "SysCall Error: Invalid filename or security violation.".to_string()
    }
}

fn handle_list_files() -> String {
    let _ = fs::create_dir_all(WORKSPACE_DIR);
    match fs::read_dir(WORKSPACE_DIR) {
        Ok(entries) => {
            let files: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            if files.is_empty() {
                "Workspace is empty.".to_string()
            } else {
                format!("Files:\n- {}", files.join("\n- "))
            }
        }
        Err(e) => format!("SysCall Error: LS failed: {}", e),
    }
}

fn handle_syscall(command_block: &str) -> String {
    // Parser pulito
    let clean_cmd = command_block.trim();

    // --- TOOL: PYTHON ---
    if clean_cmd.starts_with("PYTHON:") {
        let code = clean_cmd.trim_start_matches("PYTHON:");
        return run_python_code(code);
    }

    // --- TOOL: WRITE_FILE ---
    if clean_cmd.starts_with("WRITE_FILE:") {
        let args = clean_cmd.trim_start_matches("WRITE_FILE:");
        return handle_write_file(args);
    }

    // --- TOOL: READ_FILE ---
    if clean_cmd.starts_with("READ_FILE:") {
        let filename = clean_cmd.trim_start_matches("READ_FILE:").trim();
        return handle_read_file(filename);
    }

    // --- TOOL: LS ---
    if clean_cmd.starts_with("LS") {
        return handle_list_files();
    }

    // --- TOOL: CALC (Legacy) ---
    if clean_cmd.starts_with("CALC:") {
        let expr = clean_cmd.trim_start_matches("CALC:").trim();
        return run_python_code(&format!("print({})", expr));
    }

    "SysCall Error: Unknown Tool.".to_string()
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
        total_memory_mb: 64,
    };
    let memory = Rc::new(RefCell::new(NeuralMemory::new(mem_config).unwrap()));
    let engine_state: Arc<Mutex<Option<LLMEngine>>> = Arc::new(Mutex::new(None));

    println!(
        "Agentic OS Kernel v1.0 (Python Runtime Enabled) ready on {}",
        addr
    );

    loop {
        poll.poll(&mut events, Some(std::time::Duration::from_millis(5)))?;

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
                    if let Some(client) = clients.get_mut(&token) {
                        let mut should_close = false;
                        if event.is_readable() {
                            if handle_read(client, &memory, &engine_state, token.0) {
                                should_close = true;
                            }
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

        // SCHEDULER & SYSCALL INTERCEPTOR
        let mut lock = engine_state.lock().unwrap();
        if let Some(engine) = lock.as_mut() {
            let active_pids = engine.list_active_pids();

            for pid in active_pids {
                match engine.step_process(pid) {
                    Ok(Some((text, owner_id))) => {
                        let token = Token(owner_id);
                        if let Some(client) = clients.get_mut(&token) {
                            // Invio Output al Client (Rete)
                            client.output_buffer.extend(text.as_bytes());
                            let _ = poll.registry().reregister(
                                &mut client.stream,
                                token,
                                Interest::READABLE | Interest::WRITABLE,
                            );

                            // Accumulo per SysCall (Logica)
                            client.syscall_buffer.push_str(&text);

                            // Controllo SysCall sul buffer persistente
                            // Cerchiamo l'apertura "[["
                            if let Some(start) = client.syscall_buffer.find("[[") {
                                // Cerchiamo la chiusura "]]" DOPO l'apertura
                                if let Some(end_offset) = client.syscall_buffer[start..].find("]]")
                                {
                                    let end = start + end_offset + 2; // +2 per includere "]]"

                                    // Estraiamo il comando completo
                                    let full_command =
                                        client.syscall_buffer[start..end].to_string();
                                    // Contenuto interno (es. "PYTHON: print(1)")
                                    let content = &full_command[2..full_command.len() - 2];

                                    // Eseguiamo solo se è un tool conosciuto
                                    if content.starts_with("PYTHON:")
                                        || content.starts_with("CALC:")
                                    {
                                        println!(
                                            "OS: Intercepted SysCall from PID {}: {}",
                                            pid, full_command
                                        );

                                        let result = handle_syscall(content);

                                        // Iniettiamo il risultato
                                        let feedback = format!("\nSystem Output:\n{}\n", result);
                                        let _ = engine.inject_context(pid, &feedback);

                                        // Eseguito comando => svuota buffer
                                        client.syscall_buffer.clear();
                                    }
                                }
                            }

                            // Safety Valve (Pulizia Memoria)
                            // Se il buffer cresce troppo e non contiene un inizio di comando "[[", buttiamo via il vecchio.
                            // Questo previene memory leak su generazioni lunghe senza comandi.
                            if client.syscall_buffer.len() > 8000 {
                                if let Some(start) = client.syscall_buffer.find("[[") {
                                    // Teniamo solo da "[[" in poi
                                    let preserve = client.syscall_buffer[start..].to_string();
                                    client.syscall_buffer = preserve;
                                } else {
                                    // Nessun comando in vista, svuota tutto
                                    client.syscall_buffer.clear();
                                }
                            }
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

fn handle_read(
    client: &mut Client,
    memory: &Rc<RefCell<NeuralMemory>>,
    engine_state: &Arc<Mutex<Option<LLMEngine>>>,
    client_id: usize,
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
            Err(e) => {
                return true;
            }
        }
    }
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
                            client.output_buffer.extend(protocol::response_err(&e));
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

fn handle_write(client: &mut Client) -> bool {
    while !client.output_buffer.is_empty() {
        let (head, _) = client.output_buffer.as_slices();
        match client.stream.write(head) {
            Ok(n) => {
                client.output_buffer.drain(..n);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return false,
            Err(e) => return true,
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
    client_id: usize,
) {
    let response = match header.opcode {
        OpCode::Ping => protocol::response_ok("PONG"),
        OpCode::Load => {
            let path = String::from_utf8_lossy(&payload).trim().to_string();
            match LLMEngine::load(&path) {
                Ok(new_engine) => {
                    let mut lock = engine_state.lock().unwrap();
                    *lock = Some(new_engine);
                    protocol::response_ok("Master Model Loaded.")
                }
                Err(e) => protocol::response_err(&format!("Load Failed: {}", e)),
            }
        }
        OpCode::Exec => {
            let prompt = String::from_utf8_lossy(&payload).to_string();
            let mut lock = engine_state.lock().unwrap();
            if let Some(engine) = lock.as_mut() {
                match engine.spawn_process(&prompt, 300, client_id) {
                    // Aumentato token limit per permettere codice
                    Ok(pid) => protocol::response_ok(&format!("Process Started PID: {}", pid)),
                    Err(e) => protocol::response_err(&format!("Spawn Failed: {}", e)),
                }
            } else {
                protocol::response_err("No Model Loaded")
            }
        }
        _ => protocol::response_err("Not Implemented"),
    };
    client.output_buffer.extend(response);
}
