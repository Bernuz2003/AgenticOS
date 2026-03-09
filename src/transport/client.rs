use crate::protocol::CommandHeader;
use std::collections::HashSet;

pub enum ClientState {
    WaitingForHeader,
    ReadingBody { header: CommandHeader },
}

pub enum ParsedCommand {
    Ok {
        header: CommandHeader,
        payload: Vec<u8>,
    },
    Err(String),
}

pub struct Client {
    pub stream: mio::net::TcpStream,
    pub buffer: Vec<u8>,
    pub output_buffer: std::collections::VecDeque<u8>,
    pub state: ClientState,
    pub authenticated: bool,
    pub negotiated_protocol_version: Option<String>,
    pub enabled_capabilities: HashSet<String>,
}

impl Client {
    pub fn new(stream: mio::net::TcpStream, pre_authenticated: bool) -> Self {
        Self {
            stream,
            buffer: Vec::with_capacity(4096),
            output_buffer: std::collections::VecDeque::new(),
            state: ClientState::WaitingForHeader,
            authenticated: pre_authenticated,
            negotiated_protocol_version: None,
            enabled_capabilities: HashSet::new(),
        }
    }
}
