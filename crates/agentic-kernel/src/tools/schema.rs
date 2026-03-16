use jsonschema::JSONSchema;
use schemars::JsonSchema;
use serde_json::{Map, Value};

pub(crate) fn ensure_valid_schema(schema: &Value, label: &str) -> Result<(), String> {
    JSONSchema::compile(schema)
        .map(|_| ())
        .map_err(|err| format!("Invalid JSON schema for {label}: {err}"))
}

pub(crate) fn generated_schema<T: JsonSchema>() -> Result<Value, String> {
    let schema = schemars::schema_for!(T);
    let value = serde_json::to_value(&schema)
        .map_err(|err| format!("Failed to serialize generated schema: {err}"))?;
    Ok(normalize_object_schemas(value))
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

fn normalize_object_schemas(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(normalize_object_map(map)),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(normalize_object_schemas).collect())
        }
        scalar => scalar,
    }
}

fn normalize_object_map(mut map: Map<String, Value>) -> Map<String, Value> {
    for value in map.values_mut() {
        let next = normalize_object_schemas(value.take());
        *value = next;
    }

    if is_struct_object_schema(&map) && !map.contains_key("additionalProperties") {
        map.insert("additionalProperties".to_string(), Value::Bool(false));
    }

    map
}

fn is_struct_object_schema(map: &Map<String, Value>) -> bool {
    matches!(map.get("type"), Some(Value::String(kind)) if kind == "object")
        && (map.contains_key("properties") || map.contains_key("required"))
}
