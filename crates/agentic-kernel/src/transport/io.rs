use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use agentic_control_models::KernelEvent;

use crate::commands::execute_command;
use crate::commands::MetricsState;
use crate::memory::NeuralMemory;
use crate::model_catalog::ModelCatalog;
use crate::orchestrator::Orchestrator;
use crate::protocol;
use crate::resource_governor::ResourceGovernor;
use crate::runtime::TurnAssemblyStore;
use crate::runtimes::RuntimeRegistry;
use crate::scheduler::ProcessScheduler;
use crate::services::job_scheduler::JobScheduler;
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;

use super::{parse_available_commands, Client, ParsedCommand};

#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::collections::HashMap as TestHashMap;
#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
thread_local! {
    static TEST_SESSION_STATE: RefCell<TestHashMap<usize, TestSessionState>> =
        RefCell::new(TestHashMap::new());
}

#[cfg(test)]
struct TestSessionState {
    _db_path: PathBuf,
    storage: StorageService,
    session_registry: SessionRegistry,
}

#[cfg(test)]
fn with_test_session_state<T>(
    key: usize,
    f: impl FnOnce(&mut SessionRegistry, &mut StorageService) -> T,
) -> T {
    TEST_SESSION_STATE.with(|states| {
        let mut states = states.borrow_mut();
        let state = states.entry(key).or_insert_with(new_test_session_state);
        f(&mut state.session_registry, &mut state.storage)
    })
}

#[cfg(test)]
fn new_test_session_state() -> TestSessionState {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let db_path = std::env::temp_dir().join(format!(
        "agenticos-transport-test-{}-{unique}.db",
        std::process::id()
    ));
    let mut storage = StorageService::open(&db_path).expect("open transport test storage");
    let boot = storage
        .record_kernel_boot("transport-test")
        .expect("record transport test boot");
    let session_registry =
        SessionRegistry::load(&mut storage, boot.boot_id).expect("load transport test sessions");

    TestSessionState {
        _db_path: db_path,
        storage,
        session_registry,
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn handle_read(
    client: &mut Client,
    memory: &mut NeuralMemory,
    runtime_registry: &mut RuntimeRegistry,
    model_catalog: &mut ModelCatalog,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    client_id: usize,
    shutdown_requested: &Arc<AtomicBool>,
    in_flight: &HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    metrics: &mut MetricsState,
    auth_token: &str,
) -> bool {
    let mut tool_registry = ToolRegistry::with_builtins();
    let mut pending_events = Vec::new();
    handle_read_with_test_state(
        client,
        memory,
        runtime_registry,
        model_catalog,
        scheduler,
        orchestrator,
        client_id,
        shutdown_requested,
        in_flight,
        pending_kills,
        &mut pending_events,
        metrics,
        &mut tool_registry,
        auth_token,
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn handle_read_with_test_state(
    client: &mut Client,
    memory: &mut NeuralMemory,
    runtime_registry: &mut RuntimeRegistry,
    model_catalog: &mut ModelCatalog,
    scheduler: &mut ProcessScheduler,
    orchestrator: &mut Orchestrator,
    client_id: usize,
    shutdown_requested: &Arc<AtomicBool>,
    in_flight: &HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pending_events: &mut Vec<KernelEvent>,
    metrics: &mut MetricsState,
    tool_registry: &mut ToolRegistry,
    auth_token: &str,
) -> bool {
    let key = scheduler as *mut ProcessScheduler as usize;
    with_test_session_state(key, |session_registry, storage| {
        let mut resource_governor =
            ResourceGovernor::load(storage, crate::config::ResourceGovernorConfig::default())
                .expect("load transport test governor");
        let mut job_scheduler = JobScheduler::load(storage).expect("load transport test jobs");
        let mut turn_assembly = TurnAssemblyStore::default();
        handle_read_with_registry(
            client,
            memory,
            runtime_registry,
            &mut resource_governor,
            model_catalog,
            scheduler,
            &mut job_scheduler,
            orchestrator,
            session_registry,
            storage,
            client_id,
            shutdown_requested,
            in_flight,
            pending_kills,
            pending_events,
            metrics,
            tool_registry,
            &mut turn_assembly,
            auth_token,
            None,
        )
    })
}

#[allow(clippy::too_many_arguments)]
pub fn handle_read_with_registry(
    client: &mut Client,
    memory: &mut NeuralMemory,
    runtime_registry: &mut RuntimeRegistry,
    resource_governor: &mut ResourceGovernor,
    model_catalog: &mut ModelCatalog,
    scheduler: &mut ProcessScheduler,
    job_scheduler: &mut JobScheduler,
    orchestrator: &mut Orchestrator,
    session_registry: &mut SessionRegistry,
    storage: &mut StorageService,
    client_id: usize,
    shutdown_requested: &Arc<AtomicBool>,
    in_flight: &HashSet<u64>,
    pending_kills: &mut Vec<u64>,
    pending_events: &mut Vec<KernelEvent>,
    metrics: &mut MetricsState,
    tool_registry: &mut ToolRegistry,
    turn_assembly: &mut TurnAssemblyStore,
    auth_token: &str,
    mcp_bridge: Option<&crate::mcp::bridge::McpBridgeRuntime>,
) -> bool {
    let mut chunk = [0; 4096];
    match client.stream.read(&mut chunk) {
        Ok(0) => return true,
        Ok(n) => {
            client.buffer.extend_from_slice(&chunk[..n]);
        }
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return false,
        Err(ref e)
            if e.kind() == io::ErrorKind::ConnectionReset
                || e.kind() == io::ErrorKind::BrokenPipe =>
        {
            return true;
        }
        Err(e) => {
            tracing::error!(%e, "Read error");
            return true;
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
                runtime_registry,
                resource_governor,
                model_catalog,
                scheduler,
                job_scheduler,
                orchestrator,
                tool_registry,
                session_registry,
                storage,
                turn_assembly,
                client_id,
                shutdown_requested,
                in_flight,
                pending_kills,
                pending_events,
                metrics,
                auth_token,
                mcp_bridge,
            ),
            ParsedCommand::Err(e) => {
                let request_id = client.allocate_request_id("transport");
                client.output_buffer.extend(protocol::response_protocol_err(
                    client,
                    &request_id,
                    "BAD_HEADER",
                    protocol::schema::ERROR,
                    &e,
                ));
            }
        }
    }
    false
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
