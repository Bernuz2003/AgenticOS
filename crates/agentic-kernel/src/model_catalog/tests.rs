use super::metadata::parse_tokenizer_metadata_json;
use super::*;
use crate::backend::{TestExternalEndpointOverrideGuard, TestOpenAIConfigOverrideGuard};
use crate::config::OpenAIResponsesConfig;
use agentic_control_models::{ModelCatalogSnapshot, ModelInfoResponse};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn family_inference_from_name() {
    assert_eq!(
        metadata::infer_family_from_filename("Meta-Llama-3-8B"),
        PromptFamily::Llama
    );
    assert_eq!(
        metadata::infer_family_from_filename("Qwen2.5-14B"),
        PromptFamily::Qwen
    );
    assert_eq!(
        metadata::infer_family_from_filename("Mistral-7B"),
        PromptFamily::Mistral
    );
    assert_eq!(
        metadata::infer_family_from_filename("unknown"),
        PromptFamily::Unknown
    );
}

#[test]
fn discovers_models_recursively_in_family_subdirs() {
    let base = mk_temp_dir("agenticos_catalog_recursive");
    let models = base.join("models");
    let llama_dir = models.join("llama3.1-8b");
    let qwen_dir = models.join("qwen2.5-14b");

    fs::create_dir_all(&llama_dir).expect("create llama dir");
    fs::create_dir_all(&qwen_dir).expect("create qwen dir");

    let llama_model = llama_dir.join("Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf");
    let qwen_model = qwen_dir.join("Qwen2.5-14B-Instruct-Q4_K_M.gguf");
    fs::write(&llama_model, b"stub").expect("write llama stub");
    fs::write(&qwen_model, b"stub").expect("write qwen stub");
    fs::write(llama_dir.join("tokenizer.json"), b"{}").expect("write llama tokenizer");

    let catalog = ModelCatalog::discover(&models).expect("discover models recursively");
    assert_eq!(catalog.entries.len(), 2);

    let llama = catalog
        .entries
        .iter()
        .find(|entry| entry.family == PromptFamily::Llama)
        .expect("llama entry present");
    assert!(llama
        .id
        .contains("llama3.1-8b/Meta-Llama-3.1-8B-Instruct-Q4_K_M"));
    assert!(llama
        .tokenizer_path
        .as_ref()
        .expect("tokenizer expected")
        .ends_with("tokenizer.json"));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn metadata_sidecar_overrides_family_and_exposes_capabilities() {
    let base = mk_temp_dir("agenticos_catalog_metadata");
    let models = base.join("models");
    let qwen_dir = models.join("future-model");

    fs::create_dir_all(&qwen_dir).expect("create model dir");
    let model = qwen_dir.join("custom-model.gguf");
    fs::write(&model, b"stub").expect("write model");
    fs::write(
        qwen_dir.join("metadata.json"),
        r#"{
                "family": "Qwen",
                "backend_preference": "external-llamacpp",
                "capabilities": { "code": 0.95, "general": 0.25 }
            }"#,
    )
    .expect("write metadata");

    let catalog = ModelCatalog::discover(&models).expect("discover with metadata");
    let entry = catalog.entries.first().expect("one entry");
    assert_eq!(entry.family, PromptFamily::Qwen);
    assert_eq!(
        entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.backend_preference.as_deref()),
        Some("external-llamacpp")
    );
    assert!(entry
        .metadata_source
        .as_deref()
        .unwrap_or_default()
        .ends_with("metadata.json"));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn resolve_load_target_accepts_explicit_cloud_selector() {
    let base = mk_temp_dir("agenticos_catalog_cloud_selector");
    let models = base.join("models");
    fs::create_dir_all(&models).expect("create models dir");
    let _openai = TestOpenAIConfigOverrideGuard::set(OpenAIResponsesConfig {
        endpoint: "http://127.0.0.1:19090/v1".to_string(),
        api_key: "test-key".to_string(),
        default_model: "gpt-4.1-mini".to_string(),
        timeout_ms: 5_000,
        max_request_bytes: 256 * 1024,
        max_response_bytes: 256 * 1024,
        stream: true,
        tokenizer_path: None,
        input_price_usd_per_mtok: 0.0,
        output_price_usd_per_mtok: 0.0,
        http_referer: String::new(),
        app_title: String::new(),
    });

    let catalog = ModelCatalog::discover(&models).expect("discover empty catalog");
    let target = catalog
        .resolve_load_target("cloud:openai-responses:gpt-4.1-mini")
        .expect("resolve cloud target");

    assert_eq!(target.family(), PromptFamily::Unknown);
    assert_eq!(
        target.display_path(),
        PathBuf::from("cloud/openai-responses/gpt-4.1-mini")
    );
    assert_eq!(target.runtime_reference(), "gpt-4.1-mini");
    assert_eq!(target.remote_model_id(), Some("gpt-4.1-mini"));
    assert_eq!(target.provider_id(), Some("openai-responses"));
    assert_eq!(
        target.driver_resolution().resolved_backend_id,
        "openai-responses"
    );
    assert_eq!(
        target.driver_resolution().backend_class.as_str(),
        "remote_stateless"
    );
    match &target {
        ResolvedModelTarget::Remote(remote) => {
            assert_eq!(remote.backend_id, "openai-responses");
            assert_eq!(remote.provider_label, "OpenAI");
            assert_eq!(remote.model_spec.label, "GPT-4.1 mini");
            assert_eq!(
                remote.runtime_config.adapter_kind.as_str(),
                "openai_compatible"
            );
        }
        ResolvedModelTarget::Local(_) => panic!("expected remote target"),
    }

    let _ = fs::remove_dir_all(base);
}

#[test]
fn resolve_load_target_uses_remote_provider_default_model() {
    let base = mk_temp_dir("agenticos_catalog_cloud_default_model");
    let models = base.join("models");
    fs::create_dir_all(&models).expect("create models dir");
    let _openai = TestOpenAIConfigOverrideGuard::set(OpenAIResponsesConfig {
        endpoint: "http://127.0.0.1:19090/v1".to_string(),
        api_key: "test-key".to_string(),
        default_model: String::new(),
        timeout_ms: 5_000,
        max_request_bytes: 256 * 1024,
        max_response_bytes: 256 * 1024,
        stream: true,
        tokenizer_path: None,
        input_price_usd_per_mtok: 0.0,
        output_price_usd_per_mtok: 0.0,
        http_referer: String::new(),
        app_title: String::new(),
    });

    let catalog = ModelCatalog::discover(&models).expect("discover empty catalog");
    let target = catalog
        .resolve_load_target("cloud:openai-responses")
        .expect("resolve cloud target with provider default");

    assert_eq!(
        target.display_path(),
        PathBuf::from("cloud/openai-responses/gpt-4.1-mini")
    );
    assert_eq!(target.runtime_reference(), "gpt-4.1-mini");
    assert_eq!(
        target.driver_resolution().resolved_backend_id,
        "openai-responses"
    );

    let _ = fs::remove_dir_all(base);
}

#[test]
fn parse_tokenizer_metadata_extracts_special_tokens() {
    let parsed = parse_tokenizer_metadata_json(
        r#"{
                "added_tokens": [
                    {"content": "<|endoftext|>", "special": true},
                    {"content": "<|im_start|>", "special": true},
                    {"content": "<|im_end|>", "special": true}
                ]
            }"#,
    )
    .expect("tokenizer metadata parsed");

    let special_tokens = parsed.special_tokens.expect("special tokens present");
    assert_eq!(
        special_tokens.get("eos").map(String::as_str),
        Some("<|endoftext|>")
    );
    assert_eq!(
        special_tokens.get("im_end").map(String::as_str),
        Some("<|im_end|>")
    );
    assert!(parsed
        .stop_markers
        .as_ref()
        .is_some_and(|markers| markers.iter().any(|marker| marker == "<|im_end|>")));
}

#[test]
fn discover_uses_native_tokenizer_metadata_without_sidecar() {
    let base = mk_temp_dir("agenticos_catalog_native_tokenizer");
    let models = base.join("models");
    let qwen_dir = models.join("qwen2.5-14b");

    fs::create_dir_all(&qwen_dir).expect("create qwen dir");
    fs::write(qwen_dir.join("qwen.gguf"), b"stub").expect("write gguf stub");
    fs::write(
        qwen_dir.join("tokenizer.json"),
        r#"{
                "added_tokens": [
                    {"content": "<|endoftext|>", "special": true},
                    {"content": "<|im_start|>", "special": true},
                    {"content": "<|im_end|>", "special": true}
                ]
            }"#,
    )
    .expect("write tokenizer");

    let catalog = ModelCatalog::discover(&models).expect("discover models");
    let entry = catalog.entries.first().expect("entry present");
    assert_eq!(entry.family, PromptFamily::Qwen);
    assert_eq!(
        entry
            .metadata
            .as_ref()
            .and_then(|meta| meta.special_tokens.as_ref())
            .and_then(|tokens| tokens.get("im_end"))
            .map(String::as_str),
        Some("<|im_end|>")
    );
    assert_eq!(entry.metadata_source.as_deref(), Some("native:tokenizer"));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn metadata_capabilities_drive_routing_before_family_heuristics() {
    let base = mk_temp_dir("agenticos_catalog_routing");
    let models = base.join("models");
    let llama_dir = models.join("llama3.1-8b");
    let qwen_dir = models.join("qwen2.5-14b");

    fs::create_dir_all(&llama_dir).expect("create llama dir");
    fs::create_dir_all(&qwen_dir).expect("create qwen dir");
    fs::write(llama_dir.join("llama.gguf"), b"stub").expect("write llama");
    fs::write(qwen_dir.join("qwen.gguf"), b"stub").expect("write qwen");
    fs::write(
        llama_dir.join("metadata.json"),
        r#"{ "family": "Llama", "capabilities": { "code": 0.2 } }"#,
    )
    .expect("write llama metadata");
    fs::write(
        qwen_dir.join("metadata.json"),
        r#"{ "family": "Qwen", "capabilities": { "code": 0.9 } }"#,
    )
    .expect("write qwen metadata");

    let catalog = ModelCatalog::discover(&models).expect("discover models");
    let selected = catalog
        .select_for_workload(WorkloadClass::Code)
        .expect("select code model");
    assert_eq!(selected.family, PromptFamily::Qwen);

    let _ = fs::remove_dir_all(base);
}

#[test]
fn resolve_load_target_prefers_model_id_even_if_contains_slash() {
    let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");
    let base = mk_temp_dir("agenticos_catalog_id");
    let models = base.join("models");
    let qwen_dir = models.join("qwen2.5-14b");

    fs::create_dir_all(&qwen_dir).expect("create qwen dir");
    let qwen_model = qwen_dir.join("Qwen2.5-14B-Instruct-Q4_K_M.gguf");
    fs::write(&qwen_model, b"stub").expect("write qwen stub");

    let catalog = ModelCatalog::discover(&models).expect("discover models");
    let qwen = catalog
        .entries
        .iter()
        .find(|entry| entry.family == PromptFamily::Qwen)
        .expect("qwen entry present");

    let target = catalog
        .resolve_load_target(&qwen.id)
        .expect("resolve by id with slash");
    assert_eq!(target.display_path(), qwen_model.as_path());
    assert_eq!(target.runtime_reference(), qwen_model.to_string_lossy());
    assert_eq!(target.family(), PromptFamily::Qwen);
    assert_eq!(target.local_model_id(), Some(qwen.id.as_str()));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn parse_and_infer_workload() {
    let (hint, stripped) =
        super::workload::parse_workload_hint("capability=code; scrivi parser rust");
    assert_eq!(hint, Some(WorkloadClass::Code));
    assert_eq!(stripped, "scrivi parser rust");

    assert_eq!(infer_workload_class("rispondi breve"), WorkloadClass::Fast);
    assert_eq!(
        infer_workload_class("ragiona su questo problema"),
        WorkloadClass::Reasoning
    );
}

#[test]
fn format_list_json_exposes_models_and_routing() {
    let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");
    let base = mk_temp_dir("agenticos_catalog_json");
    let models = base.join("models");
    let llama_dir = models.join("llama3.1-8b");
    let qwen_dir = models.join("qwen2.5-14b");

    fs::create_dir_all(&llama_dir).expect("create llama dir");
    fs::create_dir_all(&qwen_dir).expect("create qwen dir");
    fs::write(llama_dir.join("meta-llama-3.1-8b.gguf"), b"stub").expect("write llama stub");
    fs::write(qwen_dir.join("qwen2.5-14b.gguf"), b"stub").expect("write qwen stub");
    fs::write(qwen_dir.join("tokenizer.json"), b"{}").expect("write tokenizer");

    let mut catalog = ModelCatalog::discover(&models).expect("discover models");
    let first_id = catalog.entries[0].id.clone();
    catalog.set_selected(&first_id).expect("select first model");

    let payload: ModelCatalogSnapshot =
        serde_json::from_str(&catalog.format_list_json()).expect("json payload");
    assert_eq!(payload.total_models, 2);
    assert!(!payload.models.is_empty());
    assert!(!payload.routing_recommendations.is_empty());
    assert!(!payload.remote_providers.is_empty());
    assert!(payload
        .remote_providers
        .iter()
        .any(|provider| provider.id == "openai-responses"));
    assert!(payload
        .models
        .iter()
        .all(|entry| entry.resolved_backend_class.is_some()));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn format_list_json_exposes_routing_source_and_score() {
    let _endpoint = TestExternalEndpointOverrideGuard::set("http://127.0.0.1:18080");
    let base = mk_temp_dir("agenticos_catalog_routing_meta_json");
    let models = base.join("models");
    let qwen_dir = models.join("future-model");

    fs::create_dir_all(&qwen_dir).expect("create model dir");
    fs::write(qwen_dir.join("custom.gguf"), b"stub").expect("write model");
    fs::write(
        qwen_dir.join("metadata.json"),
        r#"{
                "family": "Qwen",
                "backend_preference": "external-llamacpp",
                "capabilities": { "code": 0.93 }
            }"#,
    )
    .expect("write metadata");

    let catalog = ModelCatalog::discover(&models).expect("discover models");
    let payload: ModelCatalogSnapshot =
        serde_json::from_str(&catalog.format_list_json()).expect("json payload");
    let code_route = payload
        .routing_recommendations
        .iter()
        .find(|item| item.workload == "code")
        .expect("code route present");

    assert_eq!(code_route.source, "metadata-capability");
    assert_eq!(code_route.capability_key.as_deref(), Some("code"));
    assert_eq!(
        code_route.backend_preference.as_deref(),
        Some("external-llamacpp")
    );
    assert_eq!(
        code_route.resolved_backend.as_deref(),
        Some("external-llamacpp")
    );
    assert_eq!(
        code_route.resolved_backend_class.as_deref(),
        Some("resident_local")
    );
    assert_eq!(
        code_route
            .resolved_backend_capabilities
            .as_ref()
            .map(|caps| caps.tool_pause_resume),
        Some(true)
    );
    assert_eq!(code_route.driver_resolution_source, "metadata-preference");
    assert_eq!(code_route.capability_score, Some(0.93));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn format_info_json_exposes_unresolved_driver_when_no_loadable_backend_exists() {
    let _endpoint = TestExternalEndpointOverrideGuard::clear();
    let base = mk_temp_dir("agenticos_catalog_driver_info");
    let models = base.join("models");
    let mistral_dir = models.join("mistral-7b");

    fs::create_dir_all(&mistral_dir).expect("create mistral dir");
    fs::write(mistral_dir.join("mistral.gguf"), b"stub").expect("write model");
    fs::write(
        mistral_dir.join("metadata.json"),
        r#"{ "family": "Mistral" }"#,
    )
    .expect("write metadata");

    let catalog = ModelCatalog::discover(&models).expect("discover models");
    let info: ModelInfoResponse = serde_json::from_str(
        &catalog
            .format_info_json("mistral-7b/mistral")
            .expect("model info"),
    )
    .expect("json info");

    assert_eq!(info.resolved_backend, None);
    assert_eq!(info.resolved_backend_class, None);
    assert_eq!(info.resolved_backend_capabilities, None);
    assert_eq!(info.driver_resolution_source, "unresolved");
    assert!(info
        .driver_resolution_rationale
        .contains("No registered loadable driver"));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn format_info_json_exposes_unresolved_driver_for_unsupported_architecture() {
    let _endpoint = TestExternalEndpointOverrideGuard::clear();
    let base = mk_temp_dir("agenticos_catalog_qwen35_driver_info");
    let models = base.join("models");
    let qwen_dir = models.join("qwen3.5-9b");

    fs::create_dir_all(&qwen_dir).expect("create qwen dir");
    fs::write(qwen_dir.join("model.gguf"), b"stub").expect("write model");
    fs::write(
        qwen_dir.join("metadata.json"),
        r#"{ "family": "Qwen", "architecture": "qwen35" }"#,
    )
    .expect("write metadata");

    let catalog = ModelCatalog::discover(&models).expect("discover models");
    let info: ModelInfoResponse = serde_json::from_str(
        &catalog
            .format_info_json("qwen3.5-9b/model")
            .expect("model info"),
    )
    .expect("json info");

    assert_eq!(info.architecture.as_deref(), Some("qwen35"));
    assert_eq!(info.resolved_backend, None);
    assert_eq!(info.resolved_backend_class, None);
    assert_eq!(info.resolved_backend_capabilities, None);
    assert_eq!(info.driver_resolution_source, "unresolved");
    assert!(info.driver_resolution_rationale.contains("qwen35"));

    let _ = fs::remove_dir_all(base);
}

fn mk_temp_dir(prefix: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time ok")
        .as_nanos();
    std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), ts))
}
