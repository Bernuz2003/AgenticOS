use std::str;

#[derive(Debug, Clone)]
pub enum OpCode {
    Ping,           // Ping-Pong
    Load,           // Carica modello
    Exec,           // Esegui inferenza
    Unload,         // Libera memoria
    MemoryWrite,    // Scrivi tensore in VRAM
}

#[derive(Debug)]
pub struct CommandHeader {
    pub opcode: OpCode,
    pub agent_id: String,
    pub content_length: usize,
}

impl CommandHeader {
    /// Parsa la riga di intestazione: "VERB AgentID Length"
    /// Esempio: "EXEC coder_01 500"
    pub fn parse(line: &str) -> Result<Self, String> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        
        if parts.is_empty() {
            return Err("Empty header".to_string());
        }

        let opcode = match parts[0].to_uppercase().as_str() {
            "PING" => OpCode::Ping,
            "LOAD" => OpCode::Load,
            "EXEC" => OpCode::Exec,
            "KILL" => OpCode::Unload,
            "MEMw" => OpCode::MemoryWrite,
            _ => return Err(format!("Unknown opcode: {}", parts[0])),
        };

        // Gestione comandi senza argomenti (es. PING)
        let agent_id = if parts.len() > 1 { parts[1].to_string() } else { "sys".to_string() };
        
        let content_length = if parts.len() > 2 {
            parts[2].parse::<usize>().map_err(|_| "Invalid content length")?
        } else {
            0
        };

        Ok(CommandHeader {
            opcode,
            agent_id,
            content_length,
        })
    }
}

/// Helper per serializzare le risposte (sempre testo semplice per le risposte di controllo)
pub fn response_ok(msg: &str) -> Vec<u8> {
    format!("+OK {}\r\n", msg).into_bytes()
}

pub fn response_err(msg: &str) -> Vec<u8> {
    format!("-ERR {}\r\n", msg).into_bytes()
}

// Quando risponderemo con Tensori, useremo un Header simile a quello di richiesta
pub fn response_data(data: &[u8]) -> Vec<u8> {
    let header = format!("DATA raw {}\r\n", data.len());
    let mut vec = header.into_bytes();
    vec.extend_from_slice(data);
    vec
}