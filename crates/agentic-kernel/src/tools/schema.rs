use jsonschema::JSONSchema;
use serde_json::Value;

pub(crate) fn ensure_valid_schema(schema: &Value, label: &str) -> Result<(), String> {
    JSONSchema::compile(schema)
        .map(|_| ())
        .map_err(|err| format!("Invalid JSON schema for {label}: {err}"))
}

pub(crate) fn validate_value(schema: &Value, instance: &Value, label: &str) -> Result<(), String> {
    let compiled = JSONSchema::compile(schema)
        .map_err(|err| format!("Invalid JSON schema for {label}: {err}"))?;

    if let Err(errors) = compiled.validate(instance) {
        let detail = errors
            .into_iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(detail);
    }

    Ok(())
}
