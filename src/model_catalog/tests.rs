use super::metadata::{parse_gguf_metadata_map, parse_tokenizer_metadata_json};
use super::*;
use candle_core::quantized::gguf_file;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn family_inference_from_name() {
    assert_eq!(metadata::infer_family_from_filename("Meta-Llama-3-8B"), PromptFamily::Llama);
    assert_eq!(metadata::infer_family_from_filename("Qwen2.5-14B"), PromptFamily::Qwen);
    assert_eq!(metadata::infer_family_from_filename("Mistral-7B"), PromptFamily::Mistral);
    assert_eq!(metadata::infer_family_from_filename("unknown"), PromptFamily::Unknown);
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
    assert!(llama.id.contains("llama3.1-8b/Meta-Llama-3.1-8B-Instruct-Q4_K_M"));
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
        entry.metadata.as_ref().and_then(|meta| meta.backend_preference.as_deref()),
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
fn parse_gguf_metadata_extracts_architecture_and_template() {
    let mut metadata_map = HashMap::new();
    metadata_map.insert(
        "general.architecture".to_string(),
        gguf_file::Value::String("qwen2".to_string()),
    );
    metadata_map.insert(
        "tokenizer.chat_template".to_string(),
        gguf_file::Value::String("<{role}>{content}</{role}>".to_string()),
    );

    let parsed = parse_gguf_metadata_map(&metadata_map).expect("gguf metadata parsed");
    assert_eq!(parsed.family.as_deref(), Some("Qwen"));
    assert_eq!(parsed.architecture.as_deref(), Some("qwen2"));
    assert_eq!(parsed.chat_template.as_deref(), Some("<{role}>{content}</{role}>"));
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
    assert_eq!(special_tokens.get("eos").map(String::as_str), Some("<|endoftext|>"));
    assert_eq!(special_tokens.get("im_end").map(String::as_str), Some("<|im_end|>"));
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
    assert_eq!(target.path, qwen_model);
    assert_eq!(target.family, PromptFamily::Qwen);
    assert_eq!(target.model_id.as_deref(), Some(qwen.id.as_str()));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn parse_and_infer_workload() {
    let (hint, stripped) = parse_workload_hint("capability=code; scrivi parser rust");
    assert_eq!(hint, Some(WorkloadClass::Code));
    assert_eq!(stripped, "scrivi parser rust");

    assert_eq!(infer_workload_class("rispondi breve"), WorkloadClass::Fast);
    assert_eq!(infer_workload_class("ragiona su questo problema"), WorkloadClass::Reasoning);
}

#[test]
fn format_list_json_exposes_models_and_routing() {
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

    let payload: serde_json::Value = serde_json::from_str(&catalog.format_list_json()).expect("json payload");
    assert_eq!(payload["total_models"].as_u64(), Some(2));
    assert!(payload["models"].as_array().map(|items| !items.is_empty()).unwrap_or(false));
    assert!(payload["routing_recommendations"]
        .as_array()
        .map(|items| !items.is_empty())
        .unwrap_or(false));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn format_list_json_exposes_routing_source_and_score() {
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
    let payload: serde_json::Value =
        serde_json::from_str(&catalog.format_list_json()).expect("json payload");
    let code_route = payload["routing_recommendations"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["workload"].as_str() == Some("code")))
        .expect("code route present");

    assert_eq!(code_route["source"].as_str(), Some("metadata-capability"));
    assert_eq!(code_route["capability_key"].as_str(), Some("code"));
    assert_eq!(code_route["backend_preference"].as_str(), Some("external-llamacpp"));
    assert_eq!(code_route["resolved_backend"].as_str(), Some("candle.quantized_qwen2"));
    assert_eq!(
        code_route["driver_resolution_source"].as_str(),
        Some("metadata-preference-fallback")
    );
    assert_eq!(code_route["capability_score"].as_f64(), Some(0.93));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn format_info_json_exposes_unresolved_driver_when_no_loadable_backend_exists() {
    let base = mk_temp_dir("agenticos_catalog_driver_info");
    let models = base.join("models");
    let mistral_dir = models.join("mistral-7b");

    fs::create_dir_all(&mistral_dir).expect("create mistral dir");
    fs::write(mistral_dir.join("mistral.gguf"), b"stub").expect("write model");
    fs::write(mistral_dir.join("metadata.json"), r#"{ "family": "Mistral" }"#)
        .expect("write metadata");

    let catalog = ModelCatalog::discover(&models).expect("discover models");
    let info: serde_json::Value = serde_json::from_str(
        &catalog
            .format_info_json("mistral-7b/mistral")
            .expect("model info"),
    )
    .expect("json info");

    assert_eq!(info["resolved_backend"], serde_json::Value::Null);
    assert_eq!(info["driver_resolution_source"].as_str(), Some("unresolved"));
    assert!(info["driver_resolution_rationale"]
        .as_str()
        .unwrap_or_default()
        .contains("No registered loadable driver"));

    let _ = fs::remove_dir_all(base);
}

#[test]
fn format_info_json_exposes_unresolved_driver_for_unsupported_architecture() {
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
    let info: serde_json::Value = serde_json::from_str(
        &catalog
            .format_info_json("qwen3.5-9b/model")
            .expect("model info"),
    )
    .expect("json info");

    assert_eq!(info["architecture"].as_str(), Some("qwen35"));
    assert_eq!(info["resolved_backend"], serde_json::Value::Null);
    assert_eq!(info["driver_resolution_source"].as_str(), Some("unresolved"));
    assert!(info["driver_resolution_rationale"]
        .as_str()
        .unwrap_or_default()
        .contains("qwen35"));

    let _ = fs::remove_dir_all(base);
}

fn mk_temp_dir(prefix: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time ok")
        .as_nanos();
    std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), ts))
}