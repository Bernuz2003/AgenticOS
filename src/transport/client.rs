use crate::protocol::CommandHeader;

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
}

impl Client {
    pub fn new(stream: mio::net::TcpStream) -> Self {
        Self {
            stream,
            buffer: Vec::with_capacity(4096),
            output_buffer: std::collections::VecDeque::new(),
            state: ClientState::WaitingForHeader,
        }
    }
}
