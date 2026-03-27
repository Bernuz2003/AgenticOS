use super::load_remote_provider_catalog_from_path;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn loads_enabled_remote_provider_catalog_from_toml() {
    let base = std::env::temp_dir().join(format!(
        "agenticos_remote_catalog_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&base).expect("create temp dir");
    let path = base.join("remote_providers.toml");
    fs::write(
        &path,
        r#"
                [[providers]]
                id = "openai-responses"
                adapter = "openai_compatible"
                label = "OpenAI"
                default_model_id = "gpt-4.1-mini"
                enabled = true

                [[providers.models]]
                id = "gpt-4.1-mini"
                label = "GPT-4.1 mini"

                [[providers]]
                id = "disabled-provider"
                label = "Disabled"
                default_model_id = "x"
                enabled = false
            "#,
    )
    .expect("write remote providers catalog");

    let catalog = load_remote_provider_catalog_from_path(&path).expect("load catalog");
    assert_eq!(catalog.providers.len(), 1);
    assert_eq!(catalog.providers[0].id, "openai-responses");
    assert_eq!(catalog.providers[0].default_model_id, "gpt-4.1-mini");
    assert_eq!(catalog.providers[0].models[0].label, "GPT-4.1 mini");
    assert_ne!(catalog.fingerprint, 0);

    let _ = fs::remove_dir_all(base);
}
