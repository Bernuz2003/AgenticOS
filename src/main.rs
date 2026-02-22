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
use std::path::PathBuf; // Rimosso 'Path' che era inutilizzato
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
    state: ClientState,
}

impl Client {
    fn new(stream: mio::net::TcpStream) -> Self {
        Client {
            stream,
            buffer: Vec::with_capacity(4096),
            output_buffer: VecDeque::new(),
            state: ClientState::WaitingForHeader,
        }
    }
}

// --- TOOL HELPERS ---

fn run_python_code(code: &str) -> String {
    let clean_code = code
        .trim()
        .trim_start_matches("```python")
        .trim_start_matches("```")
        .trim_end_matches("```");

    println!("OS: Executing Python Code:\n---\n{}\n---", clean_code);
    let temp_filename = "agent_script_temp.py";
    if let Err(e) = std::fs::write(temp_filename, clean_code) {
        return format!("SysCall Error: Failed to write temp file: {}", e);
    }
    let output = Command::new("python3").arg(temp_filename).output();
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let result = if !stderr.is_empty() {
                format!("Output:\n{}\nErrors:\n{}", stdout, stderr)
            } else {
                format!("{}", stdout)
            };
            let _ = std::fs::remove_file(temp_filename);
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

fn resolve_safe_path(filename: &str) -> Option<PathBuf> {
    let clean_name = filename.trim();
    if clean_name.contains("..") || clean_name.starts_with('/') || clean_name.starts_with('\\') {
        return None;
    }
    let mut path = PathBuf::from(WORKSPACE_DIR);
    path.push(clean_name);
    Some(path)
}

fn handle_write_file(args: &str) -> String {
    let parts: Vec<&str> = args.splitn(2, '|').collect();
    if parts.len() < 2 {
        return "SysCall Error: Usage [[WRITE_FILE: filename | content]]".to_string();
    }
    let filename = parts[0].trim();
    let content = parts[1].trim_start();
    if let Err(e) = fs::create_dir_all(WORKSPACE_DIR) {
        return format!("SysCall Error: Failed to create workspace: {}", e);
    }
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
            Ok(content) => content,
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
    let clean_cmd = command_block.trim();
    if clean_cmd.starts_with("PYTHON:") {
        return run_python_code(clean_cmd.trim_start_matches("PYTHON:"));
    }
    if clean_cmd.starts_with("WRITE_FILE:") {
        return handle_write_file(clean_cmd.trim_start_matches("WRITE_FILE:"));
    }
    if clean_cmd.starts_with("READ_FILE:") {
        return handle_read_file(clean_cmd.trim_start_matches("READ_FILE:").trim());
    }
    if clean_cmd.starts_with("LS") {
        return handle_list_files();
    }
    if clean_cmd.starts_with("CALC:") {
        let expr = clean_cmd.trim_start_matches("CALC:").trim();
        return run_python_code(&format!("print({})", expr));
    }
    "SysCall Error: Unknown Tool.".to_string()
}

// --- HELPER PER IL FORMATO LLAMA 3 ---
fn format_system_injection(content: &str) -> String {
    format!(
        "<|eot_id|><|start_header_id|>system<|end_header_id|>\n\n{}\n<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n",
        content
    )
}

// --- MAIN LOOP ---

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
        "Agentic OS Kernel v1.3 (Process-Centric SysCalls) ready on {}",
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
                        if !should_close && !client.output_buffer.is_empty() {
                            poll.registry().reregister(
                                &mut client.stream,
                                token,
                                Interest::READABLE | Interest::WRITABLE,
                            )?;
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

        let mut lock = engine_state.lock().unwrap();
        if let Some(engine) = lock.as_mut() {
            let active_pids = engine.list_active_pids();

            for pid in active_pids {
                // 1. ESECUZIONE STEP (AI genera token)
                let step_result = engine.step_process(pid);

                match step_result {
                    Ok(Some((text, owner_id))) => {
                        // --- FASE A: UPDATE BUFFER PROCESS (Scope Ristretto) ---
                        // Usiamo una variabile per salvare l'eventuale comando da eseguire DOPO
                        // aver rilasciato il borrow di `engine.processes`.
                        let mut pending_syscall: Option<String> = None;

                        if let Some(proc) = engine.processes.get_mut(&pid) {
                            proc.syscall_buffer.push_str(&text);

                            if let Some(start) = proc.syscall_buffer.find("[[") {
                                if let Some(end_offset) = proc.syscall_buffer[start..].find("]]") {
                                    let end = start + end_offset + 2;
                                    let full_command = proc.syscall_buffer[start..end].to_string();

                                    // Svuotiamo il buffer e salviamo il comando per l'esecuzione
                                    proc.syscall_buffer.clear();
                                    pending_syscall = Some(full_command);
                                }
                            }

                            // Safety valve buffer size
                            if proc.syscall_buffer.len() > 8000 {
                                proc.syscall_buffer.clear();
                            }
                        }

                        // --- FASE B: ESECUZIONE SYSCALL (Scope Libero) ---
                        // Ora possiamo chiamare engine.spawn_process perché non abbiamo più
                        // in mano il riferimento `proc`.
                        if let Some(full_command) = pending_syscall {
                            let content =
                                full_command[2..full_command.len() - 2].trim().to_string();
                            println!(
                                "OS: SysCall from PID {} (Owner {}): {}",
                                pid, owner_id, full_command
                            );

                            if content.starts_with("SPAWN:") {
                                let prompt = content.trim_start_matches("SPAWN:").trim();
                                // I figli nascono con owner_id 0 (Daemon/Background)
                                match engine.spawn_process(prompt, 500, 0) {
                                    Ok(new_pid) => {
                                        let msg = format!("SUCCESS: Worker Created (PID {}).\nSTOP SPAWNING NEW PROCESSES.\nNEXT ACTION: Use [[SEND: {} | <your_question>]] immediately.", new_pid, new_pid);
                                        let feedback = format_system_injection(&msg);
                                        let _ = engine.inject_context(pid, &feedback);
                                    }
                                    Err(e) => {
                                        let _ = engine.inject_context(
                                            pid,
                                            &format_system_injection(&format!("ERROR: {}", e)),
                                        );
                                    }
                                }
                            } else if content.starts_with("SEND:") {
                                let parts: Vec<&str> =
                                    content.trim_start_matches("SEND:").splitn(2, '|').collect();
                                if parts.len() == 2 {
                                    let message = parts[1].trim();
                                    let target_pid_str = parts[0].trim();
                                    if let Ok(target_pid) = parts[0].trim().parse::<u64>() {
                                        // Inietta nel target
                                        let msg_target = format!("<|eot_id|><|start_header_id|>user<|end_header_id|>\n\n[Message from PID {}]: {}\n<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n", pid, message);
                                        match engine.inject_context(target_pid, &msg_target) {
                                            Ok(_) => {
                                                let _ = engine.inject_context(
                                                    pid,
                                                    &format_system_injection("MESSAGE SENT. Waiting for reply... (Do not send again)."),
                                                );
                                            }
                                            Err(_) => {
                                                let _ = engine.inject_context(pid, &format_system_injection("ERROR: Target PID not found (Process does not exist)."));
                                            }
                                        }
                                    } else {
                                        let err_msg = format!("ERROR: Invalid PID format '{}'. You must use a numeric PID (e.g., [[SEND: 2 | ...]]).", target_pid_str);
                                        let _ = engine.inject_context(
                                            pid,
                                            &format_system_injection(&err_msg),
                                        );
                                    }
                                }
                            }
                            // TOOLS (Python, VFS...)
                            else if content.starts_with("PYTHON:")
                                || content.starts_with("WRITE_FILE:")
                                || content.starts_with("READ_FILE:")
                                || content.starts_with("LS")
                                || content.starts_with("CALC:")
                            {
                                let result = handle_syscall(&content);
                                let _ = engine.inject_context(
                                    pid,
                                    &format_system_injection(&format!("Output:\n{}", result)),
                                );
                            }
                        }

                        // --- FASE C: OUTPUT UTENTE ---
                        // Inviamo al client SOLO se il processo ha un proprietario umano (id > 0)
                        if owner_id > 0 {
                            let token = Token(owner_id);
                            if let Some(client) = clients.get_mut(&token) {
                                client.output_buffer.extend(text.as_bytes());
                                let _ = poll.registry().reregister(
                                    &mut client.stream,
                                    token,
                                    Interest::READABLE | Interest::WRITABLE,
                                );
                            }
                        } else {
                            // Processo Background: silenzioso su rete
                            // print!("{}", text); // Decommentare per debug server-side
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("Error PID {}: {}", pid, e);
                        engine.kill_process(pid);
                    }
                }
            }

            let finished_pids = engine.list_finished_pids();
            for pid in finished_pids {
                if let Some(owner_id) = engine.process_owner_id(pid) {
                    if owner_id > 0 {
                        let token = Token(owner_id);
                        if let Some(client) = clients.get_mut(&token) {
                            let end_msg = format!("\n[PROCESS_FINISHED pid={}]\n", pid);
                            client.output_buffer.extend(end_msg.as_bytes());
                            let _ = poll.registry().reregister(
                                &mut client.stream,
                                token,
                                Interest::READABLE | Interest::WRITABLE,
                            );
                        }
                    }
                }
                engine.kill_process(pid);
            }
        }
    }
}

// Includi anche queste funzioni per la gestione IO (sono le stesse di prima, ma servono per compilare)
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
            Err(_e) => return true,
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
                match engine.spawn_process(&prompt, 500, client_id) {
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
