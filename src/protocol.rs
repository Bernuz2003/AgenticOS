use std::str;

#[derive(Debug, Clone)]
pub enum OpCode {
    Ping,           // Ping-Pong
    Load,           // Carica modello
    Exec,           // Esegui inferenza
    Kill,           // Termina processo immediatamente
    Term,           // Richiede terminazione graceful
    Status,         // Stato kernel/processo
    Shutdown,       // Shutdown kernel
    MemoryWrite,    // Scrivi tensore in VRAM
    ListModels,     // Lista modelli disponibili
    SelectModel,    // Seleziona modello di default
    ModelInfo,      // Mostra info modello
    SetGen,         // Configura generation params
    GetGen,         // Legge generation params
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

        if parts.len() != 3 {
            return Err("Invalid header format. Expected: <OPCODE> <AGENT_ID> <CONTENT_LENGTH>"
                .to_string());
        }

        let opcode = match parts[0].to_uppercase().as_str() {
            "PING" => OpCode::Ping,
            "LOAD" => OpCode::Load,
            "EXEC" => OpCode::Exec,
            "KILL" => OpCode::Kill,
            "TERM" => OpCode::Term,
            "STATUS" => OpCode::Status,
            "SHUTDOWN" => OpCode::Shutdown,
            "MEMW" => OpCode::MemoryWrite,
            "LIST_MODELS" => OpCode::ListModels,
            "SELECT_MODEL" => OpCode::SelectModel,
            "MODEL_INFO" => OpCode::ModelInfo,
            "SET_GEN" => OpCode::SetGen,
            "GET_GEN" => OpCode::GetGen,
            _ => return Err(format!("Unknown opcode: {}", parts[0])),
        };

        let agent_id = parts[1].to_string();

        let content_length = parts[2]
            .parse::<usize>()
            .map_err(|_| "Invalid content length")?;

        Ok(CommandHeader {
            opcode,
            agent_id,
            content_length,
        })
    }
}

/// Helper per serializzare le risposte (sempre testo semplice per le risposte di controllo)
pub fn response_ok(msg: &str) -> Vec<u8> {
    response_ok_code("GENERIC", msg)
}

pub fn response_err(msg: &str) -> Vec<u8> {
    response_err_code("GENERIC", msg)
}

pub fn response_ok_code(code: &str, msg: &str) -> Vec<u8> {
    let payload = msg.as_bytes();
    let mut out = format!("+OK {} {}\r\n", code, payload.len()).into_bytes();
    out.extend_from_slice(payload);
    out
}

pub fn response_err_code(code: &str, msg: &str) -> Vec<u8> {
    let payload = msg.as_bytes();
    let mut out = format!("-ERR {} {}\r\n", code, payload.len()).into_bytes();
    out.extend_from_slice(payload);
    out
}

// Quando risponderemo con Tensori, useremo un Header simile a quello di richiesta
pub fn response_data(data: &[u8]) -> Vec<u8> {
    let header = format!("DATA raw {}\r\n", data.len());
    let mut vec = header.into_bytes();
    vec.extend_from_slice(data);
    vec
}

#[cfg(test)]
mod tests {
    use super::{response_err_code, response_ok_code, CommandHeader, OpCode};

    #[test]
    fn parse_basic_opcodes() {
        let ping = CommandHeader::parse("PING 1 0").expect("PING should parse");
        assert!(matches!(ping.opcode, OpCode::Ping));

        let load = CommandHeader::parse("LOAD 1 10").expect("LOAD should parse");
        assert!(matches!(load.opcode, OpCode::Load));

        let status = CommandHeader::parse("STATUS 1 0").expect("STATUS should parse");
        assert!(matches!(status.opcode, OpCode::Status));

        let term = CommandHeader::parse("TERM 1 1").expect("TERM should parse");
        assert!(matches!(term.opcode, OpCode::Term));

        let kill = CommandHeader::parse("KILL 1 1").expect("KILL should parse");
        assert!(matches!(kill.opcode, OpCode::Kill));

        let shutdown =
            CommandHeader::parse("SHUTDOWN 1 0").expect("SHUTDOWN should parse");
        assert!(matches!(shutdown.opcode, OpCode::Shutdown));
    }

    #[test]
    fn parse_extended_model_opcodes() {
        let list = CommandHeader::parse("LIST_MODELS 1 0").expect("LIST_MODELS should parse");
        assert!(matches!(list.opcode, OpCode::ListModels));

        let select =
            CommandHeader::parse("SELECT_MODEL 1 7").expect("SELECT_MODEL should parse");
        assert!(matches!(select.opcode, OpCode::SelectModel));

        let info = CommandHeader::parse("MODEL_INFO 1 0").expect("MODEL_INFO should parse");
        assert!(matches!(info.opcode, OpCode::ModelInfo));

        let set_gen = CommandHeader::parse("SET_GEN 1 10").expect("SET_GEN should parse");
        assert!(matches!(set_gen.opcode, OpCode::SetGen));

        let get_gen = CommandHeader::parse("GET_GEN 1 0").expect("GET_GEN should parse");
        assert!(matches!(get_gen.opcode, OpCode::GetGen));
    }

    #[test]
    fn parse_memw_case_insensitive() {
        let memw = CommandHeader::parse("memw 1 4").expect("memw should parse");
        assert!(matches!(memw.opcode, OpCode::MemoryWrite));
    }

    #[test]
    fn parse_invalid_opcode() {
        let err = CommandHeader::parse("WHAT 1 0").expect_err("invalid opcode must fail");
        assert!(err.contains("Unknown opcode"));
    }

    #[test]
    fn parse_requires_three_tokens() {
        let err = CommandHeader::parse("PING").expect_err("header without fields must fail");
        assert!(err.contains("Invalid header format"));

        let err =
            CommandHeader::parse("PING 1 0 extra").expect_err("header with extra fields fails");
        assert!(err.contains("Invalid header format"));
    }

    #[test]
    fn coded_response_format() {
        let ok = String::from_utf8(response_ok_code("PING", "PONG")).expect("utf8 ok");
        assert!(ok.starts_with("+OK PING 4\r\n"));
        assert!(ok.ends_with("PONG"));

        let err = String::from_utf8(response_err_code("BAD_HEADER", "Malformed")).expect("utf8 ok");
        assert!(err.starts_with("-ERR BAD_HEADER 9\r\n"));
        assert!(err.ends_with("Malformed"));
    }
}