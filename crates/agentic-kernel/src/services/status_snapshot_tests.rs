use super::{
    build_global_status, checked_out_pid_status_response, collect_unique_pids,
    restored_pid_status_response, runtime_backend_status, StatusSnapshotDeps,
};
use crate::backend::{resolve_driver_for_model, TestOpenAIConfigOverrideGuard};
use crate::backend::{
    ContextSlotPersistence, InferenceBackend, InferenceStepRequest, InferenceStepResult,
    RuntimeModel,
};
use crate::commands::MetricsState;
use crate::config::OpenAIResponsesConfig;
use crate::memory::NeuralMemory;
use crate::model_catalog::{ModelCatalog, RemoteModelEntry, ResolvedModelTarget};
use crate::orchestrator::Orchestrator;
use crate::process::{ContextPolicy, ContextState, ContextStatusSnapshot, ContextStrategy};
use crate::prompting::PromptFamily;
use crate::resource_governor::ResourceGovernor;
use crate::runtimes::{RuntimeRegistry, RuntimeReservation};
use crate::scheduler::{CheckedOutProcessMetadata, ProcessScheduler, RestoredProcessMetadata};
use crate::session::SessionRegistry;
use crate::storage::StorageService;
use anyhow::Result;
use std::collections::HashSet;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[test]
fn collect_unique_pids_preserves_first_seen_order() {
    let unique = collect_unique_pids([&[1, 2, 3], &[3, 4], &[2, 5], &[]]);
    assert_eq!(unique, vec![1, 2, 3, 4, 5]);
}

struct FakeResidentBackend;

impl InferenceBackend for FakeResidentBackend {
    fn backend_id(&self) -> &'static str {
        "external-llamacpp"
    }

    fn family(&self) -> PromptFamily {
        PromptFamily::Qwen
    }

    fn generate_step(&mut self, _request: InferenceStepRequest<'_>) -> Result<InferenceStepResult> {
        panic!("generate_step should not be called in status snapshot tests");
    }

    fn duplicate_boxed(&self) -> Option<Box<dyn crate::backend::ModelBackend>> {
        None
    }
}

impl ContextSlotPersistence for FakeResidentBackend {}

#[test]
fn runtime_backend_status_reports_resident_backend_capabilities() {
    let model = RuntimeModel::from_boxed_backend(Box::new(FakeResidentBackend));

    let (backend_id, backend_class, backend_capabilities, backend_telemetry) =
        runtime_backend_status(&model);

    assert_eq!(backend_id.as_deref(), Some("external-llamacpp"));
    assert_eq!(backend_class.as_deref(), Some("resident_local"));
    assert_eq!(
        backend_capabilities
            .as_ref()
            .map(|capabilities| capabilities.persistent_slots),
        Some(true)
    );
    assert_eq!(
        backend_capabilities
            .as_ref()
            .map(|capabilities| capabilities.resident_kv),
        Some(true)
    );
    assert_eq!(backend_telemetry, None);
}

#[test]
fn checked_out_status_preserves_backend_slot_metadata() {
    let policy = ContextPolicy::new(ContextStrategy::SlidingWindow, 256, 256, 128, 4);
    let context = ContextStatusSnapshot::from_parts(&policy, &ContextState::default());
    let response = checked_out_pid_status_response(
        "sess-test-000042".to_string(),
        42,
        None,
        None,
        &CheckedOutProcessMetadata {
            owner_id: 7,
            state: "InFlight".to_string(),
            checked_out_at: Instant::now(),
            tokens: 128,
            index_pos: 64,
            max_tokens: 512,
            context_slot_id: Some(9),
            resident_slot_policy: Some("park_and_resume".to_string()),
            resident_slot_state: Some("allocated".to_string()),
            resident_slot_snapshot_path: Some("workspace/swap/pid_42_slot_9.swap".to_string()),
            backend_id: Some("external-llamacpp".to_string()),
            backend_class: Some("resident_local".to_string()),
            backend_capabilities: Some(crate::backend::BackendCapabilities {
                resident_kv: true,
                persistent_slots: true,
                save_restore_slots: true,
                prompt_cache_reuse: true,
                streaming_generation: false,
                structured_output: false,
                cancel_generation: false,
                memory_telemetry: false,
                tool_pause_resume: true,
                context_compaction_reset: true,
                parallel_sessions: true,
            }),
            context,
        },
    );

    assert_eq!(response.context_slot_id, Some(9));
    assert_eq!(
        response.resident_slot_policy.as_deref(),
        Some("park_and_resume")
    );
    assert_eq!(response.resident_slot_state.as_deref(), Some("allocated"));
    assert_eq!(
        response.resident_slot_snapshot_path.as_deref(),
        Some("workspace/swap/pid_42_slot_9.swap")
    );
    assert_eq!(response.backend_id.as_deref(), Some("external-llamacpp"));
    assert_eq!(response.backend_class.as_deref(), Some("resident_local"));
    assert_eq!(
        response
            .backend_capabilities
            .as_ref()
            .map(|capabilities| capabilities.save_restore_slots),
        Some(true)
    );
}

#[test]
fn restored_status_can_surface_absent_backend_slot_metadata() {
    let response = restored_pid_status_response(
        "sess-test-000077".to_string(),
        77,
        None,
        &RestoredProcessMetadata {
            owner_id: 3,
            state: "Restored".to_string(),
            token_count: 32,
            max_tokens: 256,
            context_slot_id: None,
            resident_slot_policy: None,
            resident_slot_state: None,
            resident_slot_snapshot_path: None,
            backend_id: None,
            backend_class: None,
            backend_capabilities: None,
            context_policy: ContextPolicy::new(ContextStrategy::Summarize, 512, 384, 192, 4),
            context_state: ContextState::default(),
        },
    );

    assert_eq!(response.context_slot_id, None);
    assert_eq!(response.resident_slot_state, None);
    assert_eq!(response.resident_slot_snapshot_path, None);
    assert_eq!(response.backend_id, None);
    assert_eq!(response.backend_class, None);
    assert_eq!(response.backend_capabilities, None);
}

fn test_openai_config() -> OpenAIResponsesConfig {
    OpenAIResponsesConfig {
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
    }
}

#[test]
fn global_status_surfaces_cloud_backend_metadata_for_lobby() {
    let _openai = TestOpenAIConfigOverrideGuard::set(test_openai_config());
    let driver_resolution =
        resolve_driver_for_model(PromptFamily::Unknown, None, Some("openai-responses"))
            .expect("resolve openai backend");
    let target = ResolvedModelTarget::remote(
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
        test_openai_config().into(),
        None,
        driver_resolution,
    );
    let memory = NeuralMemory::new().expect("memory init");
    let model_catalog =
        ModelCatalog::discover(crate::config::kernel_config().paths.models_dir.clone())
            .expect("discover model catalog");
    let scheduler = ProcessScheduler::new();
    let orchestrator = Orchestrator::new();
    let in_flight = HashSet::new();
    let metrics = MetricsState::new();
    let session_registry = fresh_session_registry();
    let (mut runtime_storage, mut runtime_registry, resource_governor) = fresh_runtime_registry();
    runtime_registry
        .activate_target(&mut runtime_storage, &target, RuntimeReservation::default())
        .expect("activate runtime");

    let status = build_global_status(&StatusSnapshotDeps {
        memory: &memory,
        runtime_registry: &runtime_registry,
        resource_governor: &resource_governor,
        model_catalog: &model_catalog,
        scheduler: &scheduler,
        orchestrator: &orchestrator,
        in_flight: &in_flight,
        metrics: &metrics,
        session_registry: &session_registry,
        storage: &runtime_storage,
    });

    assert!(status.model.loaded);
    assert_eq!(status.model.loaded_model_id, "gpt-4.1-mini");
    assert_eq!(
        status.model.loaded_target_kind.as_deref(),
        Some("remote_provider")
    );
    assert_eq!(
        status.model.loaded_provider_id.as_deref(),
        Some("openai-responses")
    );
    assert_eq!(
        status.model.loaded_remote_model_id.as_deref(),
        Some("gpt-4.1-mini")
    );
    assert_eq!(
        status.model.loaded_backend.as_deref(),
        Some("openai-responses")
    );
    assert_eq!(
        status.model.loaded_backend_class.as_deref(),
        Some("remote_stateless")
    );
    assert_eq!(
        status
            .model
            .loaded_backend_capabilities
            .as_ref()
            .map(|capabilities| capabilities.resident_kv),
        Some(false)
    );
    assert_eq!(
        status
            .model
            .loaded_remote_model
            .as_ref()
            .map(|model| model.model_id.as_str()),
        Some("gpt-4.1-mini")
    );
    assert!(status.model.loaded_backend_telemetry.is_some());
    assert_eq!(status.model.runtime_instances.len(), 1);
    assert!(status.model.resource_governor.is_some());
    assert_eq!(
        status.model.runtime_instances[0].backend_id,
        "openai-responses"
    );
}

fn fresh_session_registry() -> SessionRegistry {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let db_path = std::env::temp_dir().join(format!("agenticos-status-snapshot-{unique}.db"));
    let mut storage = StorageService::open(&db_path).expect("open session registry storage");
    let boot = storage
        .record_kernel_boot("status-snapshot-test")
        .expect("record status snapshot boot");
    SessionRegistry::load(&mut storage, boot.boot_id).expect("load session registry")
}

fn fresh_runtime_registry() -> (StorageService, RuntimeRegistry, ResourceGovernor) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let db_path = std::env::temp_dir().join(format!("agenticos-runtime-snapshot-{unique}.db"));
    let mut storage = StorageService::open(&db_path).expect("open runtime registry storage");
    let registry = RuntimeRegistry::load(&mut storage).expect("load runtime registry");
    let governor = ResourceGovernor::load(
        &mut storage,
        crate::config::ResourceGovernorConfig::default(),
    )
    .expect("load resource governor");
    (storage, registry, governor)
}
