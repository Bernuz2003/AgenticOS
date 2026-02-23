use crate::protocol::CommandHeader;

use super::{ClientState, ParsedCommand};

pub fn parse_available_commands(buffer: &mut Vec<u8>, state: &mut ClientState) -> Vec<ParsedCommand> {
    let mut parsed = Vec::new();

    loop {
        match state {
            ClientState::WaitingForHeader => {
                if let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                    let header_bytes = buffer.drain(..=pos).collect::<Vec<u8>>();
                    let header_str = String::from_utf8_lossy(&header_bytes).trim().to_string();
                    if header_str.is_empty() {
                        continue;
                    }

                    match CommandHeader::parse(&header_str) {
                        Ok(header) => {
                            if header.content_length == 0 {
                                parsed.push(ParsedCommand::Ok {
                                    header,
                                    payload: Vec::new(),
                                });
                            } else {
                                *state = ClientState::ReadingBody { header };
                            }
                        }
                        Err(e) => parsed.push(ParsedCommand::Err(e)),
                    }
                } else {
                    break;
                }
            }
            ClientState::ReadingBody { header } => {
                if buffer.len() >= header.content_length {
                    let payload = buffer.drain(..header.content_length).collect::<Vec<u8>>();
                    let copied_header = CommandHeader {
                        opcode: header.opcode.clone(),
                        agent_id: header.agent_id.clone(),
                        content_length: header.content_length,
                    };
                    parsed.push(ParsedCommand::Ok {
                        header: copied_header,
                        payload,
                    });
                    *state = ClientState::WaitingForHeader;
                } else {
                    break;
                }
            }
        }
    }

    parsed
}
