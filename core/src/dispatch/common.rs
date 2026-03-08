//! Shared dispatch utilities — JSON helpers and serde helpers.

pub fn to_json<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| format!("JSON serialization error: {}", e))
}

pub fn ok_null() -> Result<String, String> {
    Ok("null".to_string())
}

/// Deserializes `T` wrapped in `Some`. With `#[serde(default)]` on an `Option<Option<T>>`,
/// absent keys → `None`, JSON `null` → `Some(None)`, JSON value → `Some(Some(value))`.
pub fn deserialize_some<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}
