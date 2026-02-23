use mio::Interest;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::commands::execute_command;
use crate::engine::LLMEngine;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::prompting::PromptFamily;
use crate::protocol::{self, CommandHeader};

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
    pub output_buffer: VecDeque<u8>,
    pub state: ClientState,
}

impl Client {
    pub fn new(stream: mio::net::TcpStream) -> Self {
        Self {
            stream,
            buffer: Vec::with_capacity(4096),
            output_buffer: VecDeque::new(),
            state: ClientState::WaitingForHeader,
        }
    }
}

pub fn handle_read(
    client: &mut Client,
    memory: &Rc<RefCell<NeuralMemory>>,
    engine_state: &Arc<Mutex<Option<LLMEngine>>>,
    model_catalog: &mut ModelCatalog,
    active_family: &mut PromptFamily,
    client_id: usize,
    shutdown_requested: &Arc<AtomicBool>,
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

    let parsed = parse_available_commands(&mut client.buffer, &mut client.state);
    for command in parsed {
        match command {
            ParsedCommand::Ok { header, payload } => execute_command(
                client,
                header,
                payload,
                memory,
                engine_state,
                model_catalog,
                active_family,
                client_id,
                shutdown_requested,
            ),
            ParsedCommand::Err(e) => {
                client
                    .output_buffer
                    .extend(protocol::response_err_code("BAD_HEADER", &e));
            }
        }
    }
    false
}

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

pub fn handle_write(client: &mut Client) -> bool {
    while !client.output_buffer.is_empty() {
        let (head, _) = client.output_buffer.as_slices();
        match client.stream.write(head) {
            Ok(n) => {
                client.output_buffer.drain(..n);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return false,
            Err(_) => return true,
        }
    }
    false
}

pub fn needs_writable_interest(client: &Client) -> bool {
    !client.output_buffer.is_empty()
}

pub fn writable_interest() -> Interest {
    Interest::READABLE | Interest::WRITABLE
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::rc::Rc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crate::engine::LLMEngine;
    use crate::memory::{MemoryConfig, NeuralMemory};
    use crate::model_catalog::ModelCatalog;
    use crate::prompting::PromptFamily;

    use super::{handle_read, handle_write, Client};
    use super::{parse_available_commands, ClientState, ParsedCommand};

    fn setup_client_and_peer() -> (Client, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
        let addr = listener.local_addr().expect("listener local addr");

        let peer = TcpStream::connect(addr).expect("connect peer");
        peer.set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set peer timeout");

        let (server_stream, _) = listener.accept().expect("accept stream");
        server_stream
            .set_nonblocking(true)
            .expect("set nonblocking");

        let mio_stream = mio::net::TcpStream::from_std(server_stream);
        (Client::new(mio_stream), peer)
    }

    fn setup_shared_state() -> (
        Rc<RefCell<NeuralMemory>>,
        Arc<Mutex<Option<LLMEngine>>>,
        ModelCatalog,
        PromptFamily,
        Arc<AtomicBool>,
    ) {
        let memory = Rc::new(RefCell::new(
            NeuralMemory::new(MemoryConfig {
                block_size: 16,
                hidden_dim: 256,
                total_memory_mb: 64,
            })
            .expect("memory init"),
        ));
        let engine_state: Arc<Mutex<Option<LLMEngine>>> = Arc::new(Mutex::new(None));
        let catalog = ModelCatalog::discover("models").expect("catalog discover");
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        (
            memory,
            engine_state,
            catalog,
            PromptFamily::Llama,
            shutdown_requested,
        )
    }

    fn setup_shared_state_with_memory_mb(
        total_memory_mb: usize,
    ) -> (
        Rc<RefCell<NeuralMemory>>,
        Arc<Mutex<Option<LLMEngine>>>,
        ModelCatalog,
        PromptFamily,
        Arc<AtomicBool>,
    ) {
        let memory = Rc::new(RefCell::new(
            NeuralMemory::new(MemoryConfig {
                block_size: 16,
                hidden_dim: 256,
                total_memory_mb,
            })
            .expect("memory init"),
        ));
        let engine_state: Arc<Mutex<Option<LLMEngine>>> = Arc::new(Mutex::new(None));
        let catalog = ModelCatalog::discover("models").expect("catalog discover");
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        (
            memory,
            engine_state,
            catalog,
            PromptFamily::Llama,
            shutdown_requested,
        )
    }

    #[test]
    fn partial_header_waits_for_newline() {
        let mut state = ClientState::WaitingForHeader;
        let mut buffer = b"PING 1 0".to_vec();

        let parsed = parse_available_commands(&mut buffer, &mut state);
        assert!(parsed.is_empty());
        assert!(!buffer.is_empty());
    }

    #[test]
    fn parses_header_with_body_after_second_chunk() {
        let mut state = ClientState::WaitingForHeader;
        let mut buffer = b"EXEC 1 5\nhe".to_vec();

        let parsed_first = parse_available_commands(&mut buffer, &mut state);
        assert!(parsed_first.is_empty());
        assert!(matches!(state, ClientState::ReadingBody { .. }));

        buffer.extend_from_slice(b"llo");
        let parsed_second = parse_available_commands(&mut buffer, &mut state);
        assert_eq!(parsed_second.len(), 1);

        match &parsed_second[0] {
            ParsedCommand::Ok { payload, .. } => assert_eq!(payload, b"hello"),
            ParsedCommand::Err(e) => panic!("unexpected parse error: {e}"),
        }
    }

    #[test]
    fn parses_two_concatenated_commands() {
        let mut state = ClientState::WaitingForHeader;
        let mut buffer = b"PING 1 0\nPING 1 0\n".to_vec();

        let parsed = parse_available_commands(&mut buffer, &mut state);
        assert_eq!(parsed.len(), 2);
        assert!(buffer.is_empty());
        assert!(matches!(parsed[0], ParsedCommand::Ok { .. }));
        assert!(matches!(parsed[1], ParsedCommand::Ok { .. }));
    }

    #[test]
    fn invalid_header_returns_error_and_continues() {
        let mut state = ClientState::WaitingForHeader;
        let mut buffer = b"WHAT 1 0\nPING 1 0\n".to_vec();

        let parsed = parse_available_commands(&mut buffer, &mut state);
        assert_eq!(parsed.len(), 2);
        assert!(matches!(parsed[0], ParsedCommand::Err(_)));
        assert!(matches!(parsed[1], ParsedCommand::Ok { .. }));
    }

    #[test]
    fn tcp_ping_roundtrip_on_transport_layer() {
        let (mut client, mut peer) = setup_client_and_peer();
        let (memory, engine_state, mut catalog, mut family, shutdown_requested) =
            setup_shared_state();

        peer.write_all(b"PING 1 0\n").expect("write ping");

        let should_close = handle_read(
            &mut client,
            &memory,
            &engine_state,
            &mut catalog,
            &mut family,
            1,
            &shutdown_requested,
        );
        assert!(!should_close);
        assert!(!client.output_buffer.is_empty());

        let write_close = handle_write(&mut client);
        assert!(!write_close);

        let mut out = [0u8; 256];
        let n = peer.read(&mut out).expect("read ping response");
        let resp = String::from_utf8_lossy(&out[..n]);
        assert!(resp.starts_with("+OK PING 4\r\n"));
        assert!(resp.ends_with("PONG"));
    }

    #[test]
    fn tcp_partial_header_then_complete_header() {
        let (mut client, mut peer) = setup_client_and_peer();
        let (memory, engine_state, mut catalog, mut family, shutdown_requested) =
            setup_shared_state();

        peer.write_all(b"PING 1").expect("write chunk1");
        let _ = handle_read(
            &mut client,
            &memory,
            &engine_state,
            &mut catalog,
            &mut family,
            1,
            &shutdown_requested,
        );
        assert!(client.output_buffer.is_empty());

        peer.write_all(b" 0\n").expect("write chunk2");
        let _ = handle_read(
            &mut client,
            &memory,
            &engine_state,
            &mut catalog,
            &mut family,
            1,
            &shutdown_requested,
        );
        let _ = handle_write(&mut client);

        let mut out = [0u8; 256];
        let n = peer.read(&mut out).expect("read response");
        let resp = String::from_utf8_lossy(&out[..n]);
        assert!(resp.starts_with("+OK PING 4\r\n"));
        assert!(resp.ends_with("PONG"));
    }

    #[test]
    fn tcp_invalid_header_then_valid_ping_same_stream() {
        let (mut client, mut peer) = setup_client_and_peer();
        let (memory, engine_state, mut catalog, mut family, shutdown_requested) =
            setup_shared_state();

        peer.write_all(b"WHAT 1 0\nPING 1 0\n")
            .expect("write invalid+valid");
        let _ = handle_read(
            &mut client,
            &memory,
            &engine_state,
            &mut catalog,
            &mut family,
            1,
            &shutdown_requested,
        );
        let _ = handle_write(&mut client);

        let mut out = [0u8; 512];
        let n = peer.read(&mut out).expect("read combined responses");
        let resp = String::from_utf8_lossy(&out[..n]);
        assert!(resp.contains("-ERR BAD_HEADER"));
        assert!(resp.contains("+OK PING 4\r\nPONG"));
    }

    #[test]
    fn tcp_disconnect_requests_close() {
        let (mut client, peer) = setup_client_and_peer();
        let (memory, engine_state, mut catalog, mut family, shutdown_requested) =
            setup_shared_state();

        drop(peer);

        let should_close = handle_read(
            &mut client,
            &memory,
            &engine_state,
            &mut catalog,
            &mut family,
            1,
            &shutdown_requested,
        );
        assert!(should_close);
    }

    #[test]
    fn tcp_multi_client_isolated_buffers() {
        let (mut client_a, mut peer_a) = setup_client_and_peer();
        let (mut client_b, mut peer_b) = setup_client_and_peer();
        let (memory, engine_state, mut catalog, mut family, shutdown_requested) =
            setup_shared_state();

        peer_a.write_all(b"PING 1 0\n").expect("write ping a");
        peer_b.write_all(b"PING 2 0\n").expect("write ping b");

        let _ = handle_read(
            &mut client_a,
            &memory,
            &engine_state,
            &mut catalog,
            &mut family,
            1,
            &shutdown_requested,
        );
        let _ = handle_read(
            &mut client_b,
            &memory,
            &engine_state,
            &mut catalog,
            &mut family,
            2,
            &shutdown_requested,
        );

        let _ = handle_write(&mut client_a);
        let _ = handle_write(&mut client_b);

        let mut out_a = [0u8; 256];
        let n_a = peer_a.read(&mut out_a).expect("read response a");
        let resp_a = String::from_utf8_lossy(&out_a[..n_a]);
        assert!(resp_a.contains("+OK PING 4\r\nPONG"));

        let mut out_b = [0u8; 256];
        let n_b = peer_b.read(&mut out_b).expect("read response b");
        let resp_b = String::from_utf8_lossy(&out_b[..n_b]);
        assert!(resp_b.contains("+OK PING 4\r\nPONG"));
    }

    #[test]
    fn tcp_reconnect_after_disconnect_still_works() {
        let (memory, engine_state, mut catalog, mut family, shutdown_requested) =
            setup_shared_state();

        let (mut client1, peer1) = setup_client_and_peer();
        drop(peer1);
        let should_close = handle_read(
            &mut client1,
            &memory,
            &engine_state,
            &mut catalog,
            &mut family,
            1,
            &shutdown_requested,
        );
        assert!(should_close);

        let (mut client2, mut peer2) = setup_client_and_peer();
        peer2.write_all(b"PING 9 0\n").expect("write ping after reconnect");
        let should_close_2 = handle_read(
            &mut client2,
            &memory,
            &engine_state,
            &mut catalog,
            &mut family,
            9,
            &shutdown_requested,
        );
        assert!(!should_close_2);
        let _ = handle_write(&mut client2);

        let mut out = [0u8; 256];
        let n = peer2.read(&mut out).expect("read reconnect response");
        let resp = String::from_utf8_lossy(&out[..n]);
        assert!(resp.contains("+OK PING 4\r\nPONG"));
    }

    #[test]
    fn tcp_status_returns_kernel_metrics_snapshot() {
        let (mut client, mut peer) = setup_client_and_peer();
        let (memory, engine_state, mut catalog, mut family, shutdown_requested) =
            setup_shared_state();

        peer.write_all(b"STATUS 1 0\n").expect("write status");
        let should_close = handle_read(
            &mut client,
            &memory,
            &engine_state,
            &mut catalog,
            &mut family,
            1,
            &shutdown_requested,
        );
        assert!(!should_close);

        let _ = handle_write(&mut client);
        let mut out = [0u8; 512];
        let n = peer.read(&mut out).expect("read status response");
        let resp = String::from_utf8_lossy(&out[..n]);
        assert!(resp.starts_with("+OK STATUS"));
        assert!(resp.contains("total_commands="));
    }

    #[test]
    fn tcp_shutdown_sets_flag() {
        let (mut client, mut peer) = setup_client_and_peer();
        let (memory, engine_state, mut catalog, mut family, shutdown_requested) =
            setup_shared_state();

        peer.write_all(b"SHUTDOWN 1 0\n").expect("write shutdown");
        let should_close = handle_read(
            &mut client,
            &memory,
            &engine_state,
            &mut catalog,
            &mut family,
            1,
            &shutdown_requested,
        );
        assert!(!should_close);

        let _ = handle_write(&mut client);
        let mut out = [0u8; 512];
        let n = peer.read(&mut out).expect("read shutdown response");
        let resp = String::from_utf8_lossy(&out[..n]);
        assert!(resp.starts_with("+OK SHUTDOWN"));
        assert!(shutdown_requested.load(Ordering::SeqCst));
    }

    #[test]
    fn tcp_pressure_memw_queued_does_not_block_ping() {
        let (mut client, mut peer) = setup_client_and_peer();
        let (memory, engine_state, mut catalog, mut family, shutdown_requested) =
            setup_shared_state_with_memory_mb(0);

        let swap_dir = format!(
            "workspace/test_transport_swap_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        {
            let mut mem = memory.borrow_mut();
            mem.configure_async_swap(true, Some(std::path::PathBuf::from(&swap_dir)))
                .expect("enable swap");
            mem.set_token_slot_quota_per_pid(4096);
            mem.register_process(77, 512).expect("register process");
        }

        let memw_bytes = 16usize;
        let mut memw_payload = Vec::with_capacity(3 + memw_bytes);
        memw_payload.extend_from_slice(b"77\n");
        memw_payload.extend(vec![0u8; memw_bytes]);
        let memw_header = format!("MEMW 1 {}\n", memw_payload.len());

        peer.write_all(memw_header.as_bytes())
            .expect("write memw header");
        peer.write_all(&memw_payload).expect("write memw payload");

        let mut should_close_memw = false;
        for _ in 0..8 {
            should_close_memw = handle_read(
                &mut client,
                &memory,
                &engine_state,
                &mut catalog,
                &mut family,
                1,
                &shutdown_requested,
            );
            if should_close_memw || !client.output_buffer.is_empty() {
                break;
            }
        }
        assert!(!should_close_memw);
        assert!(
            !client.output_buffer.is_empty(),
            "expected MEMW response after chunked reads"
        );
        let _ = handle_write(&mut client);

        let mut out_memw = [0u8; 1024];
        let n_memw = peer.read(&mut out_memw).expect("read memw response");
        let memw_resp = String::from_utf8_lossy(&out_memw[..n_memw]);
        assert!(memw_resp.contains("+OK MEMW_QUEUED"));

        peer.write_all(b"PING 1 0\n").expect("write ping");
        let should_close_ping = handle_read(
            &mut client,
            &memory,
            &engine_state,
            &mut catalog,
            &mut family,
            1,
            &shutdown_requested,
        );
        assert!(!should_close_ping);
        let _ = handle_write(&mut client);

        let mut out_ping = [0u8; 256];
        let n_ping = peer.read(&mut out_ping).expect("read ping response");
        let ping_resp = String::from_utf8_lossy(&out_ping[..n_ping]);
        assert!(ping_resp.contains("+OK PING 4\r\nPONG"));

        let waiting = memory.borrow().snapshot().pending_swaps;
        assert!(waiting >= 1, "expected at least one pending swap job");

        let _ = std::fs::remove_dir_all(swap_dir);
    }
}
