use serde_json::{json, Value};

pub(crate) fn request(id: u64, method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
}

pub(crate) fn notification(method: &str, params: Option<Value>) -> Value {
    match params {
        Some(params) => json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }),
        None => json!({
            "jsonrpc": "2.0",
            "method": method,
        }),
    }
}

pub(crate) fn success_response(id: u64, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

pub(crate) fn error_response(id: u64, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        },
    })
}

pub(crate) fn extract_response_id(message: &Value) -> Option<u64> {
    message
        .get("id")
        .and_then(Value::as_u64)
        .filter(|_| message.get("method").is_none())
}

pub(crate) fn extract_request_method(message: &Value) -> Option<&str> {
    message
        .get("method")
        .and_then(Value::as_str)
        .filter(|_| message.get("id").is_some())
}

pub(crate) fn extract_notification_method(message: &Value) -> Option<&str> {
    message
        .get("method")
        .and_then(Value::as_str)
        .filter(|_| message.get("id").is_none())
}
