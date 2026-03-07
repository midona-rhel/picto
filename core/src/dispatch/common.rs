//! Shared dispatch utilities — JSON helpers.

pub fn to_json<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| format!("JSON serialization error: {}", e))
}

pub fn ok_null() -> Result<String, String> {
    Ok("null".to_string())
}
