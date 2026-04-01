use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use agentic_control_models::KernelEvent;
use mio::{Poll, Token};
use tokenizers::models::wordlevel::WordLevel;
use tokenizers::pre_tokenizers::whitespace::Whitespace;
use tokenizers::Tokenizer;

use crate::backend::{
    BackendCapabilities, BackendClass, DriverResolution, ExternalLlamaCppBackend, InferenceBackend,
    InferenceFinishReason, InferenceStepRequest, RuntimeModel,
};
use crate::config::{RemoteAdapterKind, RemoteProviderRuntimeConfig};
use crate::events::flush_pending_events;
use crate::inference_worker::InferenceResult;
use crate::memory::NeuralMemory;
use crate::model_catalog::{ModelCatalog, RemoteModelEntry, ResolvedModelTarget, WorkloadClass};
use crate::orchestrator::Orchestrator;
use crate::process::{ProcessLifecyclePolicy, ProcessState};
use crate::prompting::{GenerationConfig, PromptFamily};
use crate::runtime::syscalls::{drain_syscall_results, SyscallCmd, SyscallCompletion};
use crate::runtime::{drain_worker_results, TurnAssemblyStore};
use crate::runtimes::{RuntimeRegistry, RuntimeReservation};
use crate::scheduler::{CheckedOutProcessMetadata, ProcessPriority, ProcessScheduler};
use crate::services::process_runtime::{spawn_managed_process_with_session, ManagedProcessRequest};
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use crate::tool_registry::ToolRegistry;
use crate::tools::invocation::{ProcessPermissionPolicy, ToolCaller};
use crate::tools::SysCallOutcome;
use crate::transport::Client;

use super::helpers::{create_temp_dir, remove_temp_dir};

#[derive(Debug, Clone)]
pub struct MockLocalCompletionChunk {
    pub content: String,
    pub stop: bool,
}

#[derive(Debug, Clone)]
pub struct LocalBackendStreamObservation {
    pub request_prompt: String,
    pub emitted_text: String,
    pub observed_chunks: Vec<String>,
    pub generated_tokens: usize,
    pub finished: bool,
}

pub fn run_local_backend_stream(
    chunks: &[MockLocalCompletionChunk],
) -> Result<LocalBackendStreamObservation, String> {
    let tokenizer = test_tokenizer();
    let (endpoint, handle) = spawn_mock_llamacpp_stream_server(chunks);
    let mut backend =
        ExternalLlamaCppBackend::for_diagnostics(endpoint, PromptFamily::Llama, 5_000, 128);
    let generation = GenerationConfig::defaults_for(PromptFamily::Unknown);
    let mut observed_chunks = Vec::new();
    let step = backend
        .generate_step(InferenceStepRequest {
            context_slot_id: None,
            tokens: &[1],
            rendered_prompt: "hello",
            resident_prompt_suffix: "hello",
            index_pos: 0,
            remaining_generation_budget: generation.max_tokens,
            tokenizer: &tokenizer,
            generation,
            stream_observer: Some(&mut |chunk: &str| observed_chunks.push(chunk.to_string())),
            eos_token_id: 2,
            eot_token_id: 3,
        })
        .map_err(|err| err.to_string())?;

    handle
        .join()
        .map_err(|_| "mock llama.cpp server thread panicked".to_string())?;

    Ok(LocalBackendStreamObservation {
        request_prompt: "hello".to_string(),
        emitted_text: step.emitted_text,
        observed_chunks,
        generated_tokens: step.appended_tokens.len(),
        finished: step.finished,
    })
}

pub fn run_live_local_completion(prompt: &str) -> Result<LocalBackendStreamObservation, String> {
    let _ = crate::config::initialize().map_err(|err| err.to_string())?;
    let target = resolve_live_local_target()?;
    let family = target.family();
    let generation = GenerationConfig::defaults_for(family);
    let rendered_prompt = prompt.to_string();
    run_live_local_completion_with_request(&rendered_prompt, family, generation)
}

pub fn run_live_local_exec_completion(
    prompt: &str,
) -> Result<LocalBackendStreamObservation, String> {
    let _ = crate::config::initialize().map_err(|err| err.to_string())?;
    let target = resolve_live_local_target()?;
    let family = target.family();
    let generation = GenerationConfig::defaults_for(family);
    let system_prompt = crate::agent_prompt::build_agent_system_prompt(
        &ToolRegistry::with_builtins(),
        ToolCaller::AgentText,
    );
    let rendered_prompt = crate::prompting::format_initial_prompt_with_metadata(
        Some(&system_prompt),
        prompt,
        family,
        target.metadata(),
    );
    run_live_local_completion_with_request(&rendered_prompt, family, generation)
}

fn run_live_local_completion_with_request(
    rendered_prompt: &str,
    family: PromptFamily,
    generation: GenerationConfig,
) -> Result<LocalBackendStreamObservation, String> {
    let chunk_tokens = crate::config::kernel_config()
        .external_llamacpp
        .chunk_tokens
        .max(1);
    let endpoint = live_local_endpoint_for_family(family)?;
    let mut backend = ExternalLlamaCppBackend::for_diagnostics(
        endpoint,
        family,
        crate::config::kernel_config().external_llamacpp.timeout_ms,
        chunk_tokens,
    );
    let tokenizer = test_tokenizer();
    let mut observed_chunks = Vec::new();
    let step = backend
        .generate_step(InferenceStepRequest {
            context_slot_id: None,
            tokens: &[1],
            rendered_prompt: &rendered_prompt,
            resident_prompt_suffix: &rendered_prompt,
            index_pos: 0,
            remaining_generation_budget: generation.max_tokens,
            tokenizer: &tokenizer,
            generation,
            stream_observer: Some(&mut |chunk: &str| observed_chunks.push(chunk.to_string())),
            eos_token_id: 2,
            eot_token_id: 3,
        })
        .map_err(|err| err.to_string())?;

    Ok(LocalBackendStreamObservation {
        request_prompt: rendered_prompt.to_string(),
        emitted_text: step.emitted_text,
        observed_chunks,
        generated_tokens: step.appended_tokens.len(),
        finished: step.finished,
    })
}

pub fn run_live_remote_completion(
    backend_id: &str,
    model_reference: &str,
    prompt: &str,
) -> Result<LocalBackendStreamObservation, String> {
    let _ = crate::config::initialize().map_err(|err| err.to_string())?;
    let tokenizer = test_tokenizer();
    let generation = GenerationConfig::defaults_for(PromptFamily::Unknown);
    let mut model =
        RuntimeModel::load_from_reference(model_reference, PromptFamily::Unknown, backend_id)
            .map_err(|err| err.to_string())?;
    let mut observed_chunks = Vec::new();
    let step = model
        .generate_step(InferenceStepRequest {
            context_slot_id: None,
            tokens: &[1],
            rendered_prompt: prompt,
            resident_prompt_suffix: prompt,
            index_pos: 0,
            remaining_generation_budget: generation.max_tokens,
            tokenizer: &tokenizer,
            generation,
            stream_observer: Some(&mut |chunk: &str| observed_chunks.push(chunk.to_string())),
            eos_token_id: 2,
            eot_token_id: 3,
        })
        .map_err(|err| err.to_string())?;

    Ok(LocalBackendStreamObservation {
        request_prompt: prompt.to_string(),
        emitted_text: step.emitted_text,
        observed_chunks,
        generated_tokens: step.appended_tokens.len(),
        finished: step.finished,
    })
}

pub struct KernelE2eHarness {
    _db_dir: std::path::PathBuf,
    runtime_id: String,
    runtime_registry: RuntimeRegistry,
    memory: NeuralMemory,
    scheduler: ProcessScheduler,
    orchestrator: Orchestrator,
    session_registry: SessionRegistry,
    storage: StorageService,
    turn_assembly: TurnAssemblyStore,
    pending_events: Vec<KernelEvent>,
    in_flight: HashSet<u64>,
    pending_kills: Vec<u64>,
    result_tx: mpsc::Sender<InferenceResult>,
    result_rx: mpsc::Receiver<InferenceResult>,
    syscall_cmd_tx: mpsc::Sender<SyscallCmd>,
    syscall_cmd_rx: mpsc::Receiver<SyscallCmd>,
    syscall_result_tx: mpsc::Sender<SyscallCompletion>,
    syscall_result_rx: mpsc::Receiver<SyscallCompletion>,
    tool_registry: ToolRegistry,
    poll: Poll,
    clients: HashMap<Token, Client>,
    next_event_sequence: u64,
}

impl KernelE2eHarness {
    pub fn new() -> Result<Self, String> {
        let db_dir = create_temp_dir("agenticos-e2e-harness").map_err(|err| err.to_string())?;
        let db_path = db_dir.join("agenticos.db");
        let mut storage = StorageService::open(&db_path).map_err(|err| err.to_string())?;
        let boot = storage
            .record_kernel_boot("0.5.0-test")
            .map_err(|err| err.to_string())?;
        let mut runtime_registry =
            RuntimeRegistry::load(&mut storage).map_err(|err| err.to_string())?;
        let runtime = runtime_registry
            .activate_target(
                &mut storage,
                &remote_target(),
                RuntimeReservation::default(),
            )
            .map_err(|err| err.to_string())?;
        let (result_tx, result_rx) = mpsc::channel();
        let (syscall_cmd_tx, syscall_cmd_rx) = mpsc::channel();
        let (syscall_result_tx, syscall_result_rx) = mpsc::channel();

        Ok(Self {
            _db_dir: db_dir,
            runtime_id: runtime.runtime_id,
            runtime_registry,
            memory: NeuralMemory::new().map_err(|err| err.to_string())?,
            scheduler: ProcessScheduler::new(),
            orchestrator: Orchestrator::new(),
            session_registry: SessionRegistry::load(&mut storage, boot.boot_id)
                .map_err(|err| err.to_string())?,
            storage,
            turn_assembly: TurnAssemblyStore::default(),
            pending_events: Vec::new(),
            in_flight: HashSet::new(),
            pending_kills: Vec::new(),
            result_tx,
            result_rx,
            syscall_cmd_tx,
            syscall_cmd_rx,
            syscall_result_tx,
            syscall_result_rx,
            tool_registry: ToolRegistry::with_builtins(),
            poll: Poll::new().map_err(|err| err.to_string())?,
            clients: HashMap::new(),
            next_event_sequence: 0,
        })
    }

    pub fn spawn_interactive_process(&mut self, prompt: &str) -> Result<u64, String> {
        let runtime_id = self.runtime_id.clone();
        let spawned = {
            let pid_floor = self.runtime_registry.next_pid_floor();
            let engine = self
                .runtime_registry
                .engine_mut(&runtime_id)
                .ok_or_else(|| "runtime engine missing".to_string())?;
            spawn_managed_process_with_session(
                &runtime_id,
                pid_floor,
                engine,
                &mut self.memory,
                &mut self.scheduler,
                &mut self.session_registry,
                &mut self.storage,
                ManagedProcessRequest {
                    prompt: prompt.to_string(),
                    system_prompt: None,
                    owner_id: 7,
                    tool_caller: ToolCaller::AgentText,
                    permission_policy: Some(ProcessPermissionPolicy::interactive_chat(
                        &self.tool_registry,
                    )?),
                    workload: WorkloadClass::Fast,
                    required_backend_class: None,
                    priority: ProcessPriority::Normal,
                    lifecycle_policy: ProcessLifecyclePolicy::Interactive,
                    context_policy: None,
                },
            )?
        };
        self.runtime_registry
            .register_pid(&mut self.storage, &runtime_id, spawned.pid)
            .map_err(|err| err.to_string())?;
        let turn_id = self
            .storage
            .start_session_turn(
                &spawned.session_id,
                spawned.pid,
                "fast",
                "e2e_harness",
                prompt,
                "prompt",
            )
            .map_err(|err| err.to_string())?;
        self.session_registry
            .remember_active_turn(spawned.pid, turn_id);
        self.record_checked_out(spawned.pid)?;
        self.in_flight.insert(spawned.pid);
        Ok(spawned.pid)
    }

    pub fn send_stream_chunk(
        &self,
        pid: u64,
        text: impl Into<String>,
        first_chunk: bool,
    ) -> Result<(), String> {
        self.result_tx
            .send(InferenceResult::StreamChunk {
                pid,
                text: text.into(),
                first_chunk,
            })
            .map_err(|err| err.to_string())
    }

    pub fn send_finished_token(
        &mut self,
        pid: u64,
        text_output: impl Into<String>,
    ) -> Result<(), String> {
        self.send_token_result(
            pid,
            text_output,
            0,
            true,
            Some(InferenceFinishReason::ModelStop),
        )
    }

    pub fn send_token_result(
        &mut self,
        pid: u64,
        text_output: impl Into<String>,
        generated_tokens: usize,
        finished: bool,
        finish_reason: Option<InferenceFinishReason>,
    ) -> Result<(), String> {
        let mut process = self.take_process(pid)?;
        process.state = if finished {
            ProcessState::WaitingForInput
        } else {
            ProcessState::Running
        };
        self.result_tx
            .send(InferenceResult::Token {
                pid,
                process: Box::new(process),
                text_output: text_output.into(),
                generated_tokens,
                finished,
                finish_reason,
                accounting_event: None,
            })
            .map_err(|err| err.to_string())
    }

    pub fn drain_worker(&mut self) -> usize {
        drain_worker_results(
            &mut self.runtime_registry,
            &mut self.memory,
            &mut self.clients,
            &self.poll,
            &mut self.scheduler,
            &mut self.orchestrator,
            &self.result_rx,
            &self.syscall_cmd_tx,
            &mut self.session_registry,
            &mut self.storage,
            &mut self.turn_assembly,
            &mut self.in_flight,
            &mut self.pending_kills,
            &mut self.pending_events,
            &self.tool_registry,
        )
    }

    pub fn send_syscall_completion(
        &self,
        pid: u64,
        tool_call_id: impl Into<String>,
        command: impl Into<String>,
        output: impl Into<String>,
        success: bool,
        should_kill_process: bool,
    ) -> Result<(), String> {
        self.syscall_result_tx
            .send(SyscallCompletion {
                pid,
                tool_call_id: tool_call_id.into(),
                command: command.into(),
                caller: ToolCaller::AgentText,
                outcome: SysCallOutcome {
                    output: output.into(),
                    success,
                    duration_ms: 0,
                    should_kill_process,
                },
            })
            .map_err(|err| err.to_string())
    }

    pub fn drain_syscalls(&mut self) -> usize {
        drain_syscall_results(
            &mut self.runtime_registry,
            &mut self.memory,
            &mut self.scheduler,
            &mut self.session_registry,
            &mut self.storage,
            &self.syscall_result_rx,
            &mut self.pending_events,
        )
    }

    pub fn flush_events(&mut self) {
        flush_pending_events(
            &mut self.clients,
            &self.poll,
            &mut self.next_event_sequence,
            &mut self.session_registry,
            &mut self.storage,
            &mut self.turn_assembly,
            &mut self.pending_events,
        );
    }

    pub fn process_state_label(&self, pid: u64) -> Option<String> {
        self.runtime_registry
            .engine(&self.runtime_id)
            .and_then(|engine| engine.processes.get(&pid))
            .map(|process| format!("{:?}", process.state))
    }

    pub fn checked_out_pending_syscall(&self, pid: u64) -> Option<String> {
        self.turn_assembly
            .pending_syscall(pid)
            .map(ToString::to_string)
    }

    pub fn queued_syscall(&mut self) -> Option<(u64, String, String)> {
        match self.syscall_cmd_rx.try_recv().ok()? {
            SyscallCmd::Execute {
                pid,
                tool_call_id,
                content,
                ..
            } => Some((pid, tool_call_id, content)),
            SyscallCmd::Shutdown => None,
        }
    }

    pub fn pending_events(&self) -> Vec<KernelEvent> {
        self.pending_events.clone()
    }

    pub fn session_id_for_pid(&self, pid: u64) -> Option<String> {
        self.session_registry
            .session_id_for_pid(pid)
            .map(ToString::to_string)
    }

    pub fn replay_messages(
        &self,
        session_id: &str,
    ) -> Result<Vec<(String, String, String)>, String> {
        self.storage
            .load_replay_messages_for_session(session_id)
            .map(|messages| {
                messages
                    .into_iter()
                    .map(|message| (message.role, message.kind, message.content))
                    .collect()
            })
            .map_err(|err| err.to_string())
    }

    pub fn recent_audit_kinds(&self, pid: u64) -> Result<Vec<String>, String> {
        let session_id = self
            .session_id_for_pid(pid)
            .ok_or_else(|| format!("missing session id for pid {pid}"))?;
        self.storage
            .recent_audit_events_for_session(&session_id, 64)
            .map(|events| events.into_iter().map(|event| event.kind).collect())
            .map_err(|err| err.to_string())
    }

    pub fn prompt_text(&self, pid: u64) -> Option<String> {
        self.runtime_registry
            .engine(&self.runtime_id)
            .and_then(|engine| engine.processes.get(&pid))
            .map(|process| process.prompt_text().to_string())
    }

    fn record_checked_out(&mut self, pid: u64) -> Result<(), String> {
        let engine = self
            .runtime_registry
            .engine(&self.runtime_id)
            .ok_or_else(|| "runtime engine missing".to_string())?;
        let process = engine
            .processes
            .get(&pid)
            .ok_or_else(|| format!("missing process {pid}"))?;
        self.scheduler.record_checked_out_process(
            pid,
            CheckedOutProcessMetadata {
                owner_id: process.owner_id,
                tool_caller: process.tool_caller.clone(),
                permission_policy: process.permission_policy.clone(),
                state: format!("{:?}", process.state),
                checked_out_at: Instant::now(),
                tokens: process.tokens.len(),
                index_pos: process.index_pos,
                max_tokens: process.max_tokens,
                context_slot_id: process.context_slot_id,
                resident_slot_policy: process.resident_slot_policy_label(),
                resident_slot_state: process.resident_slot_state_label(),
                resident_slot_snapshot_path: process
                    .resident_slot_snapshot_path()
                    .map(|path| path.display().to_string()),
                backend_id: Some(engine.loaded_backend_id().to_string()),
                backend_class: Some(engine.loaded_backend_class().as_str().to_string()),
                backend_capabilities: Some(engine.loaded_backend_capabilities()),
                context: process.context_status_snapshot(),
                pending_human_request: process.pending_human_request.clone(),
            },
        );
        Ok(())
    }

    fn take_process(&mut self, pid: u64) -> Result<crate::process::AgentProcess, String> {
        self.runtime_registry
            .engine_mut(&self.runtime_id)
            .ok_or_else(|| "runtime engine missing".to_string())?
            .processes
            .remove(&pid)
            .ok_or_else(|| format!("missing process {pid}"))
    }
}

impl Drop for KernelE2eHarness {
    fn drop(&mut self) {
        remove_temp_dir(&self._db_dir);
    }
}

fn spawn_mock_llamacpp_stream_server(
    chunks: &[MockLocalCompletionChunk],
) -> (crate::backend::HttpEndpoint, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock llama.cpp server");
    let address = listener.local_addr().expect("mock llama.cpp addr");
    let chunks = chunks.to_vec();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept mock stream request");
        let mut buffer = [0_u8; 4096];
        let _ = stream.read(&mut buffer);

        let response_body = chunks
            .iter()
            .map(|chunk| {
                format!(
                    "data: {{\"content\":{},\"stop\":{}}}\n\n",
                    serde_json::to_string(&chunk.content).expect("serialize content"),
                    if chunk.stop { "true" } else { "false" }
                )
            })
            .collect::<String>();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n{}\r\n0\r\n\r\n",
            response_body.len(),
            response_body,
        );
        let _ = stream.write_all(response.as_bytes());
    });

    (
        crate::backend::HttpEndpoint::parse(&format!("http://{}", address)).expect("http endpoint"),
        handle,
    )
}

fn test_tokenizer() -> Tokenizer {
    let vocab = [
        ("<unk>".to_string(), 0),
        ("hello".to_string(), 1),
        ("world".to_string(), 2),
        ("TOOL:calc".to_string(), 3),
    ]
    .into_iter()
    .collect();

    let model = WordLevel::builder()
        .vocab(vocab)
        .unk_token("<unk>".to_string())
        .build()
        .expect("build tokenizer");

    let mut tokenizer = Tokenizer::new(model);
    tokenizer.with_pre_tokenizer(Some(Whitespace));
    tokenizer
}

fn remote_target() -> ResolvedModelTarget {
    ResolvedModelTarget::remote(
        "openai-responses",
        "OpenAI",
        "openai-responses",
        "gpt-4.1-mini",
        RemoteModelEntry {
            id: "gpt-4.1-mini".to_string(),
            label: "GPT-4.1 mini".to_string(),
            context_window_tokens: None,
            max_output_tokens: None,
            supports_structured_output: true,
            input_price_usd_per_mtok: None,
            output_price_usd_per_mtok: None,
        },
        RemoteProviderRuntimeConfig {
            backend_id: "openai-responses".to_string(),
            adapter_kind: RemoteAdapterKind::OpenAICompatible,
            endpoint: "http://127.0.0.1:19090/v1".to_string(),
            api_key: "test-key".to_string(),
            default_model: "gpt-4.1-mini".to_string(),
            timeout_ms: 5_000,
            max_request_bytes: 256 * 1024,
            max_response_bytes: 256 * 1024,
            stream: true,
            tokenizer_path: None,
            input_price_usd_per_mtok: 1.0,
            output_price_usd_per_mtok: 2.0,
            http_referer: String::new(),
            app_title: String::new(),
        },
        None,
        DriverResolution {
            resolved_backend_id: "openai-responses".to_string(),
            backend_class: BackendClass::RemoteStateless,
            capabilities: BackendCapabilities {
                streaming_generation: true,
                structured_output: true,
                ..Default::default()
            },
            resolution_source: "test-harness",
            resolution_rationale: "integration harness remote target".to_string(),
            available: true,
            load_supported: true,
        },
    )
}

fn resolve_live_local_target() -> Result<ResolvedModelTarget, String> {
    let catalog = ModelCatalog::discover(crate::config::kernel_config().paths.models_dir.clone())
        .map_err(|err| err.to_string())?;

    for preferred_family in [
        PromptFamily::Qwen,
        PromptFamily::Llama,
        PromptFamily::Mistral,
    ] {
        let mut candidates = catalog
            .entries
            .iter()
            .filter(|entry| entry.family == preferred_family)
            .collect::<Vec<_>>();
        candidates.sort_by_key(|entry| {
            std::cmp::Reverse(
                entry
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.max_context_tokens)
                    .unwrap_or(0),
            )
        });

        for entry in candidates {
            let target = catalog
                .resolve_entry_target(entry)
                .map_err(|err| err.to_string())?;
            if is_live_local_target_supported(&target) {
                return Ok(target);
            }
        }
    }

    for entry in &catalog.entries {
        let target = catalog
            .resolve_entry_target(entry)
            .map_err(|err| err.to_string())?;
        if is_live_local_target_supported(&target) {
            return Ok(target);
        }
    }

    Err(
        "No local external-llamacpp model with metadata.max_context_tokens is available."
            .to_string(),
    )
}

fn is_live_local_target_supported(target: &ResolvedModelTarget) -> bool {
    matches!(
        target,
        ResolvedModelTarget::Local(local)
            if local.driver_resolution.resolved_backend_id == "external-llamacpp"
                && local
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.max_context_tokens)
                    .is_some()
    )
}

fn live_local_endpoint_for_family(
    family: PromptFamily,
) -> Result<crate::backend::HttpEndpoint, String> {
    let base = crate::config::kernel_config().external_llamacpp.port_base;
    let port = base.saturating_add(match family {
        PromptFamily::Qwen => 0,
        PromptFamily::Llama => 1,
        PromptFamily::Mistral => 2,
        PromptFamily::Unknown => 90,
    });
    crate::backend::HttpEndpoint::parse(&format!("http://127.0.0.1:{port}"))
        .map_err(|err| err.to_string())
}
