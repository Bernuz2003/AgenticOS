use agentic_protocol::MAX_CONTENT_LENGTH;
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::commands::MetricsState;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::model_catalog::WorkloadClass;
use crate::orchestrator::Orchestrator;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use jsonschema::JSONSchema;
use serde_json::Value;

use super::{handle_read, handle_read_with_test_state, handle_write, Client};
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
    (Client::new(mio_stream, true), peer)
}

fn read_frame(peer: &mut TcpStream) -> String {
    let mut frame = Vec::new();
    let mut expected_total_len = None;

    loop {
        let mut chunk = [0u8; 4096];
        let n = peer.read(&mut chunk).expect("read frame");
        frame.extend_from_slice(&chunk[..n]);

        if expected_total_len.is_none() {
            if let Some(header_end) = frame.windows(2).position(|window| window == b"\r\n") {
                let header = String::from_utf8_lossy(&frame[..header_end]);
                let payload_len = header
                    .split_whitespace()
                    .last()
                    .and_then(|value| value.parse::<usize>().ok())
                    .expect("frame payload length");
                expected_total_len = Some(header_end + 2 + payload_len);
            }
        }

        if let Some(expected_total_len) = expected_total_len {
            if frame.len() >= expected_total_len {
                break;
            }
        }
    }

    String::from_utf8_lossy(&frame).to_string()
}

#[allow(clippy::too_many_arguments)]
fn pump_read_with_registry(
    client: &mut Client,
    memory: &mut NeuralMemory,
    engine_state: &mut RuntimeRegistry,
    catalog: &mut ModelCatalog,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    shutdown_requested: &Arc<AtomicBool>,
    in_flight: &HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    metrics: &mut MetricsState,
    tool_registry: &mut ToolRegistry,
    auth_token: &str,
) {
    let mut pending_events = Vec::new();
    for _ in 0..4 {
        let _ = handle_read_with_test_state(
            client,
            memory,
            engine_state,
            catalog,
            scheduler,
            orchestrator,
            1,
            shutdown_requested,
            in_flight,
            pending_kills,
            &mut pending_events,
            metrics,
            tool_registry,
            auth_token,
        );
        if !client.output_buffer.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn pump_write(client: &mut Client) {
    for _ in 0..4 {
        let _ = handle_write(client);
        if client.output_buffer.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn control_payload(resp: &str) -> &str {
    resp.split_once("\r\n")
        .map(|(_, payload)| payload)
        .expect("framed payload")
}

fn control_json(resp: &str) -> Value {
    serde_json::from_str(control_payload(resp)).expect("valid json payload")
}

fn repository_path(relative: &str) -> PathBuf {
    crate::config::repository_path(relative)
}

fn load_schema(rel_path: &str) -> Value {
    let path = repository_path(rel_path);
    load_schema_from_path(&path)
}

fn load_schema_from_path(path: &std::path::Path) -> Value {
    let text = fs::read_to_string(path).expect("schema file");
    let mut schema: Value = serde_json::from_str(&text).expect("schema json");
    resolve_local_refs(path.parent().expect("schema parent"), &mut schema);
    strip_schema_ids(&mut schema);
    schema
}

fn resolve_local_refs(base_dir: &std::path::Path, node: &mut Value) {
    match node {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref") {
                if let Some(relative_path) = reference.strip_prefix("./") {
                    let ref_path = base_dir.join(relative_path);
                    *node = load_schema_from_path(&ref_path);
                    return;
                }
            }

            for value in map.values_mut() {
                resolve_local_refs(base_dir, value);
            }
        }
        Value::Array(items) => {
            for item in items {
                resolve_local_refs(base_dir, item);
            }
        }
        _ => {}
    }
}

fn strip_schema_ids(node: &mut Value) {
    match node {
        Value::Object(map) => {
            map.remove("$id");
            for value in map.values_mut() {
                strip_schema_ids(value);
            }
        }
        Value::Array(items) => {
            for item in items {
                strip_schema_ids(item);
            }
        }
        _ => {}
    }
}

fn assert_matches_schema(schema_rel_path: &str, instance: &Value) {
    let schema = load_schema(schema_rel_path);
    let compiled = JSONSchema::compile(&schema).expect("compile schema");
    let validation = compiled
        .validate(instance)
        .map_err(|errors| errors.map(|item| item.to_string()).collect::<Vec<_>>());
    if let Err(details) = validation {
        panic!("schema validation failed: {}", details.join(" | "));
    }
}

fn parse_control_json_frames(resp: &str) -> Vec<Value> {
    let bytes = resp.as_bytes();
    let mut offset = 0usize;
    let mut payloads = Vec::new();

    while offset < bytes.len() {
        let remaining = &bytes[offset..];
        let Some(header_end) = remaining.windows(2).position(|window| window == b"\r\n") else {
            break;
        };
        let header = std::str::from_utf8(&remaining[..header_end]).expect("utf8 header");
        let parts: Vec<&str> = header.split_whitespace().collect();
        assert!(parts.len() >= 3, "invalid frame header: {header}");
        let payload_len = parts[2].parse::<usize>().expect("payload len");
        let payload_start = offset + header_end + 2;
        let payload_end = payload_start + payload_len;
        let payload =
            std::str::from_utf8(&bytes[payload_start..payload_end]).expect("utf8 payload");
        payloads.push(serde_json::from_str::<Value>(payload).expect("json payload"));
        offset = payload_end;
    }

    payloads
}

#[allow(clippy::type_complexity)]
fn setup_shared_state() -> (
    NeuralMemory,
    RuntimeRegistry,
    ModelCatalog,
    Arc<AtomicBool>,
    ProcessScheduler,
    Orchestrator,
    HashSet<u64>,
    Vec<u64>,
    MetricsState,
) {
    let memory = NeuralMemory::new().expect("memory init");
    let engine_state = fresh_runtime_registry();
    let catalog = ModelCatalog::discover(repository_path("models")).expect("catalog discover");
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    (
        memory,
        engine_state,
        catalog,
        shutdown_requested,
        ProcessScheduler::new(),
        Orchestrator::new(),
        HashSet::new(),
        Vec::new(),
        MetricsState::new(),
    )
}

#[allow(clippy::type_complexity)]
fn setup_shared_state_for_swap_pressure() -> (
    NeuralMemory,
    RuntimeRegistry,
    ModelCatalog,
    Arc<AtomicBool>,
    ProcessScheduler,
    Orchestrator,
    HashSet<u64>,
    Vec<u64>,
    MetricsState,
) {
    let memory = NeuralMemory::new().expect("memory init");
    let engine_state = fresh_runtime_registry();
    let catalog = ModelCatalog::discover(repository_path("models")).expect("catalog discover");
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    (
        memory,
        engine_state,
        catalog,
        shutdown_requested,
        ProcessScheduler::new(),
        Orchestrator::new(),
        HashSet::new(),
        Vec::new(),
        MetricsState::new(),
    )
}

fn fresh_runtime_registry() -> RuntimeRegistry {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let db_path = std::env::temp_dir().join(format!("agenticos-transport-runtime-{unique}.db"));
    let mut storage = StorageService::open(&db_path).expect("open runtime storage");
    RuntimeRegistry::load(&mut storage).expect("load runtime registry")
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
fn oversized_header_returns_error_and_continues() {
    let mut state = ClientState::WaitingForHeader;
    let mut buffer = format!("PING 1 {}\nPING 1 0\n", MAX_CONTENT_LENGTH + 1).into_bytes();

    let parsed = parse_available_commands(&mut buffer, &mut state);
    assert_eq!(parsed.len(), 2);
    assert!(matches!(parsed[0], ParsedCommand::Err(_)));
    assert!(matches!(parsed[1], ParsedCommand::Ok { .. }));
}

#[test]
fn tcp_ping_roundtrip_on_transport_layer() {
    let (mut client, mut peer) = setup_client_and_peer();
    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    peer.write_all(b"PING 1 0\n").expect("write ping");

    let should_close = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "test_token",
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
    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    peer.write_all(b"PING 1").expect("write chunk1");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "test_token",
    );
    assert!(client.output_buffer.is_empty());

    peer.write_all(b" 0\n").expect("write chunk2");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "test_token",
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
    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    peer.write_all(b"WHAT 1 0\nPING 1 0\n")
        .expect("write invalid+valid");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "test_token",
    );
    let _ = handle_write(&mut client);

    let mut out = [0u8; 512];
    let n = peer.read(&mut out).expect("read combined responses");
    let resp = String::from_utf8_lossy(&out[..n]);
    assert!(resp.contains("-ERR BAD_HEADER"));
    assert!(resp.contains("+OK PING 4\r\nPONG"));
}

#[test]
fn tcp_oversized_header_then_valid_ping_same_stream() {
    let (mut client, mut peer) = setup_client_and_peer();
    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    let request = format!("PING 1 {}\nPING 1 0\n", MAX_CONTENT_LENGTH + 1);
    peer.write_all(request.as_bytes())
        .expect("write oversized+valid");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "test_token",
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
    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    drop(peer);

    let should_close = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "test_token",
    );
    assert!(should_close);
}

#[test]
fn tcp_multi_client_isolated_buffers() {
    let (mut client_a, mut peer_a) = setup_client_and_peer();
    let (mut client_b, mut peer_b) = setup_client_and_peer();
    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    peer_a.write_all(b"PING 1 0\n").expect("write ping a");
    peer_b.write_all(b"PING 2 0\n").expect("write ping b");

    let _ = handle_read(
        &mut client_a,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "test_token",
    );
    let _ = handle_read(
        &mut client_b,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        2,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "test_token",
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
    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    let (mut client1, peer1) = setup_client_and_peer();
    drop(peer1);
    let should_close = handle_read(
        &mut client1,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "test_token",
    );
    assert!(should_close);

    let (mut client2, mut peer2) = setup_client_and_peer();
    peer2
        .write_all(b"PING 9 0\n")
        .expect("write ping after reconnect");
    let should_close_2 = handle_read(
        &mut client2,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        9,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "test_token",
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
    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    peer.write_all(b"STATUS 1 0\n").expect("write status");
    let should_close = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "test_token",
    );
    assert!(!should_close);

    let _ = handle_write(&mut client);
    let mut out = [0u8; 512];
    let n = peer.read(&mut out).expect("read status response");
    let resp = String::from_utf8_lossy(&out[..n]);
    assert!(resp.starts_with("+OK STATUS"));
    assert!(resp.contains("\"total_commands\""));
}

#[test]
fn tcp_shutdown_sets_flag() {
    let (mut client, mut peer) = setup_client_and_peer();
    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    peer.write_all(b"SHUTDOWN 1 0\n").expect("write shutdown");
    let should_close = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "test_token",
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
fn tcp_hello_contract_negotiates_v1_envelope() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let mut peer = TcpStream::connect(addr).expect("connect");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (server_stream, _) = listener.accept().expect("accept");
    server_stream.set_nonblocking(true).unwrap();
    let mio_stream = mio::net::TcpStream::from_std(server_stream);
    let mut client = Client::new(mio_stream, false);

    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    let payload = br#"{"supported_versions":["v1"],"required_capabilities":[]}"#;
    let header = format!("HELLO 1 {}\n", payload.len());
    peer.write_all(header.as_bytes())
        .expect("write hello header");
    peer.write_all(payload).expect("write hello payload");

    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);

    let resp = read_frame(&mut peer);
    assert!(resp.starts_with("+OK HELLO"));
    let payload = control_json(&resp);
    assert_matches_schema("protocol/schemas/v1/hello.schema.json", &payload);
    assert_eq!(payload["protocol_version"], "v1");
    assert_eq!(payload["schema_id"], "agenticos.control.hello.v1");
    assert_eq!(payload["ok"], true);
    assert_eq!(payload["data"]["negotiated_version"], "v1");
}

#[test]
fn tcp_tool_info_contract_uses_envelope_after_hello_and_auth() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let mut peer = TcpStream::connect(addr).expect("connect");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (server_stream, _) = listener.accept().expect("accept");
    server_stream.set_nonblocking(true).unwrap();
    let mio_stream = mio::net::TcpStream::from_std(server_stream);
    let mut client = Client::new(mio_stream, false);

    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    let hello_payload = br#"{"supported_versions":["v1"],"required_capabilities":[]}"#;
    let hello_header = format!("HELLO 1 {}\n", hello_payload.len());
    peer.write_all(hello_header.as_bytes())
        .expect("write hello header");
    peer.write_all(hello_payload).expect("write hello payload");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);
    let _ = read_frame(&mut peer);

    peer.write_all(b"AUTH 1 12\nsecret_token")
        .expect("write auth");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);
    let _ = read_frame(&mut peer);

    peer.write_all(b"TOOL_INFO 1 6\npython")
        .expect("write tool info");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);

    let resp = read_frame(&mut peer);
    assert!(resp.starts_with("+OK TOOL_INFO"));
    let payload = control_json(&resp);
    assert_matches_schema("protocol/schemas/v1/tool-info.schema.json", &payload);
    assert_eq!(payload["schema_id"], "agenticos.control.tool_info.v1");
    assert_eq!(payload["ok"], true);
    assert!(payload["request_id"]
        .as_str()
        .unwrap_or("")
        .starts_with("1:"));
    assert_eq!(payload["data"]["tool"]["descriptor"]["name"], "python");
    assert_eq!(payload["data"]["tool"]["backend"]["kind"], "host");
    assert!(payload["data"]["sandbox"].is_object());
}

#[test]
fn tcp_list_tools_contract_uses_envelope_after_hello_and_auth() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let mut peer = TcpStream::connect(addr).expect("connect");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (server_stream, _) = listener.accept().expect("accept");
    server_stream.set_nonblocking(true).unwrap();
    let mio_stream = mio::net::TcpStream::from_std(server_stream);
    let mut client = Client::new(mio_stream, false);

    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    let hello_payload = br#"{"supported_versions":["v1"],"required_capabilities":[]}"#;
    let hello_header = format!("HELLO 1 {}\n", hello_payload.len());
    peer.write_all(hello_header.as_bytes())
        .expect("write hello header");
    peer.write_all(hello_payload).expect("write hello payload");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);
    let _ = read_frame(&mut peer);

    peer.write_all(b"AUTH 1 12\nsecret_token")
        .expect("write auth");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);
    let _ = read_frame(&mut peer);

    peer.write_all(b"LIST_TOOLS 1 0\n")
        .expect("write list tools");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);

    let resp = read_frame(&mut peer);
    assert!(resp.starts_with("+OK LIST_TOOLS"));
    let payload = control_json(&resp);
    assert_matches_schema("protocol/schemas/v1/list-tools.schema.json", &payload);
    assert_eq!(payload["schema_id"], "agenticos.control.list_tools.v1");
    assert_eq!(payload["ok"], true);
    assert!(payload["data"]["total_tools"].as_u64().unwrap_or(0) >= 5);
    assert!(payload["data"]["tools"].is_array());
    assert!(payload["data"]["tools"][0]["descriptor"]["name"].is_string());
    assert!(payload["data"]["tools"][0]["backend"]["kind"].is_string());
}

#[test]
fn tcp_register_and_unregister_tool_contracts_use_envelope_after_hello_and_auth() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let mut peer = TcpStream::connect(addr).expect("connect");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (server_stream, _) = listener.accept().expect("accept");
    server_stream.set_nonblocking(true).unwrap();
    let mio_stream = mio::net::TcpStream::from_std(server_stream);
    let mut client = Client::new(mio_stream, false);

    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();
    let mut tool_registry = ToolRegistry::with_builtins();

    let hello_payload = br#"{"supported_versions":["v1"],"required_capabilities":[]}"#;
    let hello_header = format!("HELLO 1 {}\n", hello_payload.len());
    peer.write_all(hello_header.as_bytes())
        .expect("write hello header");
    peer.write_all(hello_payload).expect("write hello payload");
    pump_read_with_registry(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        &mut tool_registry,
        "secret_token",
    );
    pump_write(&mut client);
    let _ = read_frame(&mut peer);

    peer.write_all(b"AUTH 1 12\nsecret_token")
        .expect("write auth");
    pump_read_with_registry(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        &mut tool_registry,
        "secret_token",
    );
    pump_write(&mut client);
    let _ = read_frame(&mut peer);

    let register_payload = br#"{"descriptor":{"name":"runtime_echo","aliases":["ECHO"],"description":"Echo payload through remote backend.","input_schema":{"type":"object"},"output_schema":{"type":"object"},"backend_kind":"remote_http","capabilities":["echo"],"dangerous":false,"enabled":true,"source":"runtime"},"backend":{"kind":"remote_http","url":"http://127.0.0.1:8081/tool","method":"POST","timeout_ms":1500,"headers":{}}}"#;
    let register_header = format!("REGISTER_TOOL 1 {}\n", register_payload.len());
    let mut register_frame = register_header.into_bytes();
    register_frame.extend_from_slice(register_payload);
    peer.write_all(&register_frame)
        .expect("write register frame");
    pump_read_with_registry(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        &mut tool_registry,
        "secret_token",
    );
    pump_write(&mut client);

    let register_resp = read_frame(&mut peer);
    assert!(register_resp.starts_with("+OK REGISTER_TOOL"));
    let register_json = control_json(&register_resp);
    assert_matches_schema(
        "protocol/schemas/v1/register-tool.schema.json",
        &register_json,
    );
    assert_eq!(
        register_json["data"]["tool"]["descriptor"]["name"],
        "runtime_echo"
    );
    assert_eq!(
        register_json["data"]["tool"]["backend"]["kind"],
        "remote_http"
    );

    let unregister_payload = br#"{"name":"runtime_echo"}"#;
    let unregister_header = format!("UNREGISTER_TOOL 1 {}\n", unregister_payload.len());
    let mut unregister_frame = unregister_header.into_bytes();
    unregister_frame.extend_from_slice(unregister_payload);
    peer.write_all(&unregister_frame)
        .expect("write unregister frame");
    pump_read_with_registry(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        &mut tool_registry,
        "secret_token",
    );
    pump_write(&mut client);

    let unregister_resp = read_frame(&mut peer);
    assert!(unregister_resp.starts_with("+OK UNREGISTER_TOOL"));
    let unregister_json = control_json(&unregister_resp);
    assert_matches_schema(
        "protocol/schemas/v1/unregister-tool.schema.json",
        &unregister_json,
    );
    assert_eq!(
        unregister_json["data"]["tool"]["descriptor"]["name"],
        "runtime_echo"
    );
}

#[test]
fn tcp_register_tool_requires_hello_capability_negotiation() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let mut peer = TcpStream::connect(addr).expect("connect");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (server_stream, _) = listener.accept().expect("accept");
    server_stream.set_nonblocking(true).unwrap();
    let mio_stream = mio::net::TcpStream::from_std(server_stream);
    let mut client = Client::new(mio_stream, false);

    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();
    let mut tool_registry = ToolRegistry::with_builtins();

    peer.write_all(b"AUTH 1 12\nsecret_token")
        .expect("write auth");
    pump_read_with_registry(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        &mut tool_registry,
        "secret_token",
    );
    pump_write(&mut client);
    let _ = read_frame(&mut peer);

    let register_payload = br#"{"descriptor":{"name":"runtime_echo","aliases":[],"description":"Echo payload through remote backend.","input_schema":{"type":"object"},"output_schema":{"type":"object"},"backend_kind":"remote_http","capabilities":["echo"],"dangerous":false,"enabled":true,"source":"runtime"},"backend":{"kind":"remote_http","url":"http://127.0.0.1:8081/tool","method":"POST","timeout_ms":1500,"headers":{}}}"#;
    let register_header = format!("REGISTER_TOOL 1 {}\n", register_payload.len());
    let mut register_frame = register_header.into_bytes();
    register_frame.extend_from_slice(register_payload);
    peer.write_all(&register_frame)
        .expect("write register frame");
    pump_read_with_registry(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        &mut tool_registry,
        "secret_token",
    );
    pump_write(&mut client);

    let resp = read_frame(&mut peer);
    assert!(resp.starts_with("-ERR CAPABILITY_REQUIRED"));
    assert!(resp.contains("tool_registry_v1"));
}

#[test]
fn tcp_get_quota_contract_uses_envelope_after_hello() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let mut peer = TcpStream::connect(addr).expect("connect");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (server_stream, _) = listener.accept().expect("accept");
    server_stream.set_nonblocking(true).unwrap();
    let mio_stream = mio::net::TcpStream::from_std(server_stream);
    let mut client = Client::new(mio_stream, false);

    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();
    scheduler.register(
        77,
        WorkloadClass::General,
        crate::scheduler::ProcessPriority::High,
    );

    let hello_payload = br#"{"supported_versions":["v1"],"required_capabilities":[]}"#;
    let hello_header = format!("HELLO 1 {}\n", hello_payload.len());
    peer.write_all(hello_header.as_bytes())
        .expect("write hello header");
    peer.write_all(hello_payload).expect("write hello payload");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);
    let _ = read_frame(&mut peer);

    peer.write_all(b"AUTH 1 12\nsecret_token")
        .expect("write auth");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);
    let _ = read_frame(&mut peer);

    peer.write_all(b"GET_QUOTA 1 2\n77")
        .expect("write get_quota");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);

    let resp = read_frame(&mut peer);
    assert!(resp.starts_with("+OK GET_QUOTA"));
    let payload = control_json(&resp);
    assert_matches_schema(
        "protocol/schemas/v1/scheduler-control.schema.json",
        &payload,
    );
    assert_eq!(payload["schema_id"], "agenticos.control.get_quota.v1");
    assert_eq!(payload["data"]["pid"], 77);
    assert_eq!(payload["data"]["priority"], "high");
}

#[test]
fn tcp_bad_header_after_hello_uses_error_envelope_with_unique_request_id() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let mut peer = TcpStream::connect(addr).expect("connect");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (server_stream, _) = listener.accept().expect("accept");
    server_stream.set_nonblocking(true).unwrap();
    let mio_stream = mio::net::TcpStream::from_std(server_stream);
    let mut client = Client::new(mio_stream, false);

    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    let hello_payload = br#"{"supported_versions":["v1"],"required_capabilities":[]}"#;
    let hello_header = format!("HELLO 1 {}\n", hello_payload.len());
    peer.write_all(hello_header.as_bytes())
        .expect("write hello header");
    peer.write_all(hello_payload).expect("write hello payload");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);
    let _ = read_frame(&mut peer);

    peer.write_all(b"WHAT 1 0\nTOOL_INFO 1 0\n")
        .expect("write malformed and tool_info");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);

    let combined = read_frame(&mut peer);
    let frames: Vec<&str> = combined.split("-ERR ").collect();
    assert!(!frames.is_empty());
    assert!(combined.contains("BAD_HEADER"));
    assert!(combined.contains("AUTH_REQUIRED"));

    let payloads = parse_control_json_frames(&combined);
    assert_eq!(payloads.len(), 2);
    assert_matches_schema("protocol/schemas/v1/error.schema.json", &payloads[0]);
    assert_matches_schema("protocol/schemas/v1/error.schema.json", &payloads[1]);
    assert_ne!(payloads[0]["request_id"], payloads[1]["request_id"]);
    assert_eq!(payloads[0]["code"], "BAD_HEADER");
    assert_eq!(payloads[1]["code"], "AUTH_REQUIRED");
}

#[test]
fn tcp_pressure_memw_queued_does_not_block_ping() {
    let (mut client, mut peer) = setup_client_and_peer();
    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state_for_swap_pressure();

    let swap_dir = format!(
        "workspace/test_transport_swap_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    {
        memory
            .configure_async_swap(true, Some(std::path::PathBuf::from(&swap_dir)))
            .expect("enable swap");
        memory.set_token_slot_quota_per_pid(4096);
        memory.register_process(77, 512).expect("register process");
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
            &mut memory,
            &mut engine_state,
            &mut catalog,
            &mut scheduler,
            &mut orchestrator,
            1,
            &shutdown_requested,
            &in_flight,
            &mut pending_kills,
            &mut metrics,
            "test_token",
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
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "test_token",
    );
    assert!(!should_close_ping);
    let _ = handle_write(&mut client);

    let mut out_ping = [0u8; 256];
    let n_ping = peer.read(&mut out_ping).expect("read ping response");
    let ping_resp = String::from_utf8_lossy(&out_ping[..n_ping]);
    assert!(ping_resp.contains("+OK PING 4\r\nPONG"));

    let waiting = memory.snapshot().pending_swaps;
    assert!(waiting >= 1, "expected at least one pending swap job");

    let _ = std::fs::remove_dir_all(swap_dir);
}

#[test]
fn tcp_auth_rejects_command_before_authentication() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let peer = TcpStream::connect(addr).expect("connect");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (server_stream, _) = listener.accept().expect("accept");
    server_stream.set_nonblocking(true).unwrap();
    let mio_stream = mio::net::TcpStream::from_std(server_stream);
    // Client starts unauthenticated
    let mut client = Client::new(mio_stream, false);
    let mut peer = peer;

    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    // Send STATUS without AUTH first — should get AUTH_REQUIRED
    peer.write_all(b"STATUS 1 0\n").expect("write status");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);
    let mut out = [0u8; 512];
    let n = peer.read(&mut out).expect("read auth_required");
    let resp = String::from_utf8_lossy(&out[..n]);
    assert!(
        resp.contains("-ERR AUTH_REQUIRED"),
        "expected AUTH_REQUIRED, got: {}",
        resp
    );

    // Now authenticate with correct token
    peer.write_all(b"AUTH 1 12\nsecret_token")
        .expect("write auth");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);
    let n2 = peer.read(&mut out).expect("read auth response");
    let resp2 = String::from_utf8_lossy(&out[..n2]);
    assert!(
        resp2.contains("+OK AUTH"),
        "expected +OK AUTH, got: {}",
        resp2
    );

    // Now STATUS should work
    peer.write_all(b"STATUS 1 0\n").expect("write status 2");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);
    let n3 = peer.read(&mut out).expect("read status");
    let resp3 = String::from_utf8_lossy(&out[..n3]);
    assert!(
        resp3.contains("+OK STATUS"),
        "expected +OK STATUS, got: {}",
        resp3
    );
}

#[test]
fn tcp_auth_bad_token_rejected() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let peer = TcpStream::connect(addr).expect("connect");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (server_stream, _) = listener.accept().expect("accept");
    server_stream.set_nonblocking(true).unwrap();
    let mio_stream = mio::net::TcpStream::from_std(server_stream);
    let mut client = Client::new(mio_stream, false);
    let mut peer = peer;

    let (
        mut memory,
        mut engine_state,
        mut catalog,
        shutdown_requested,
        mut scheduler,
        mut orchestrator,
        in_flight,
        mut pending_kills,
        mut metrics,
    ) = setup_shared_state();

    // Send AUTH with wrong token
    peer.write_all(b"AUTH 1 9\nwrong_tok")
        .expect("write bad auth");
    let _ = handle_read(
        &mut client,
        &mut memory,
        &mut engine_state,
        &mut catalog,
        &mut scheduler,
        &mut orchestrator,
        1,
        &shutdown_requested,
        &in_flight,
        &mut pending_kills,
        &mut metrics,
        "secret_token",
    );
    let _ = handle_write(&mut client);
    let mut out = [0u8; 512];
    let n = peer.read(&mut out).expect("read");
    let resp = String::from_utf8_lossy(&out[..n]);
    assert!(
        resp.contains("-ERR AUTH_FAILED"),
        "expected AUTH_FAILED, got: {}",
        resp
    );
    assert!(!client.authenticated);
}

#[test]
fn workflow_control_schema_files_validate_examples() {
    let workflow_definition = serde_json::json!({
        "failure_policy": "fail_fast",
        "tasks": [
            {
                "id": "plan",
                "role": "planner",
                "prompt": "Produce a plan",
                "deps": []
            },
            {
                "id": "draft",
                "role": "writer",
                "prompt": "Write a draft",
                "deps": ["plan"],
                "allow_actions": false,
                "allowed_tools": ["read_file"]
            }
        ]
    });
    assert_matches_schema(
        "protocol/schemas/v1/workflow-definition.schema.json",
        &workflow_definition,
    );

    let list_orchestrations = serde_json::json!({
        "protocol_version": "v1",
        "schema_id": "agenticos.control.list_orchestrations.v1",
        "request_id": "req-1",
        "ok": true,
        "code": "LIST_ORCHESTRATIONS",
        "data": {
            "orchestrations": [
                {
                    "orchestration_id": 11,
                    "total": 2,
                    "completed": 1,
                    "running": 1,
                    "pending": 0,
                    "failed": 0,
                    "skipped": 0,
                    "finished": false,
                    "elapsed_secs": 3.5,
                    "policy": "FailFast"
                }
            ]
        },
        "error": null,
        "warnings": []
    });
    assert_matches_schema(
        "protocol/schemas/v1/list-orchestrations.schema.json",
        &list_orchestrations,
    );

    let orchestration_status: Value = serde_json::from_str(
        r#"{
            "protocol_version": "v1",
            "schema_id": "agenticos.control.orchestration_status.v1",
            "request_id": "req-2",
            "ok": true,
            "code": "ORCHESTRATION_STATUS",
            "data": {
                "orchestration_id": 11,
                "total": 2,
                "completed": 1,
                "running": 1,
                "pending": 0,
                "failed": 0,
                "skipped": 0,
                "finished": false,
                "elapsed_secs": 4.2,
                "policy": "FailFast",
                "truncations": 0,
                "output_chars_stored": 128,
                "tasks": [
                    {
                        "task": "plan",
                        "role": "planner",
                        "workload": "general",
                        "backend_class": "remote_stateless",
                        "context_strategy": "sliding",
                        "deps": [],
                        "status": "completed",
                        "current_attempt": 1,
                        "pid": null,
                        "error": null,
                        "termination_reason": "completed",
                        "context": null,
                        "latest_output_preview": "Plan output",
                        "latest_output_text": "Plan output",
                        "latest_output_truncated": false,
                        "input_artifacts": [],
                        "output_artifacts": [
                            {
                                "artifact_id": "art-1",
                                "task": "plan",
                                "attempt": 1,
                                "kind": "task_output",
                                "label": "plan attempt 1",
                                "mime_type": "text/markdown",
                                "preview": "Plan output",
                                "content": "Plan output",
                                "bytes": 11,
                                "created_at_ms": 1000
                            }
                        ],
                        "attempts": [
                            {
                                "attempt": 1,
                                "status": "completed",
                                "session_id": "sess-1",
                                "pid": 77,
                                "error": null,
                                "output_preview": "Plan output",
                                "output_chars": 11,
                                "truncated": false,
                                "termination_reason": "completed",
                                "started_at_ms": 900,
                                "completed_at_ms": 1000,
                                "primary_artifact_id": "art-1"
                            }
                        ]
                    }
                ],
                "ipc_messages": []
            },
            "error": null,
            "warnings": []
        }"#,
    )
    .expect("valid orchestration status example");
    assert_matches_schema(
        "protocol/schemas/v1/orchestration-status.schema.json",
        &orchestration_status,
    );

    let list_jobs: Value = serde_json::from_str(
        r#"{
            "protocol_version": "v1",
            "schema_id": "agenticos.control.list_jobs.v1",
            "request_id": "req-3",
            "ok": true,
            "code": "LIST_JOBS",
            "data": {
                "jobs": [
                    {
                        "job_id": 9,
                        "name": "nightly-review",
                        "target_kind": "workflow",
                        "trigger_kind": "cron",
                        "trigger_label": "0 * * * *",
                        "enabled": true,
                        "state": "idle",
                        "next_run_at_ms": 1700000000000,
                        "current_trigger_at_ms": null,
                        "current_attempt": 0,
                        "timeout_ms": 30000,
                        "max_retries": 2,
                        "backoff_ms": 5000,
                        "last_run_started_at_ms": null,
                        "last_run_completed_at_ms": null,
                        "last_run_status": null,
                        "last_error": null,
                        "consecutive_failures": 0,
                        "active_orchestration_id": null,
                        "recent_runs": [
                            {
                                "run_id": 4,
                                "trigger_at_ms": 1699999999000,
                                "attempt": 1,
                                "status": "completed",
                                "started_at_ms": 1699999999000,
                                "completed_at_ms": 1700000000000,
                                "orchestration_id": 22,
                                "deadline_at_ms": 1700000030000,
                                "error": null
                            }
                        ]
                    }
                ]
            },
            "error": null,
            "warnings": []
        }"#,
    )
    .expect("valid job list example");
    assert_matches_schema("protocol/schemas/v1/list-jobs.schema.json", &list_jobs);

    let list_artifacts = serde_json::json!({
        "protocol_version": "v1",
        "schema_id": "agenticos.control.list_artifacts.v1",
        "request_id": "req-4",
        "ok": true,
        "code": "LIST_ARTIFACTS",
        "data": {
            "orchestration_id": 11,
            "task": "draft",
            "artifacts": [
                {
                    "artifact_id": "art-2",
                    "task": "draft",
                    "attempt": 1,
                    "kind": "task_output",
                    "label": "draft attempt 1",
                    "mime_type": "text/markdown",
                    "preview": "Draft preview",
                    "content": "Draft full text",
                    "bytes": 15,
                    "created_at_ms": 2000
                }
            ]
        },
        "error": null,
        "warnings": []
    });
    assert_matches_schema(
        "protocol/schemas/v1/list-artifacts.schema.json",
        &list_artifacts,
    );
}
