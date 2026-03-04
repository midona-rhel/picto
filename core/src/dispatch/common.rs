//! Shared dispatch utilities — JSON helpers, argument parsing, convenience wrappers.

/// Convenience wrapper: fetch sidebar counts from bitmaps.
pub fn sidebar_counts_from_bitmaps(
    db: &crate::sqlite::SqliteDatabase,
) -> crate::events::SidebarCounts {
    crate::events::sidebar_counts_from_bitmaps(db)
}

/// Convert snake_case to camelCase: "folder_id" → "folderId"
pub fn snake_to_camel(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
    for ch in s.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Look up a key in args, trying snake_case first then camelCase.
pub fn get_field<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    args.get(key).filter(|v| !v.is_null()).or_else(|| {
        let camel = snake_to_camel(key);
        if camel != key {
            args.get(&camel).filter(|v| !v.is_null())
        } else {
            None
        }
    })
}

pub fn de<T: serde::de::DeserializeOwned>(
    args: &serde_json::Value,
    key: &str,
) -> Result<T, String> {
    get_field(args, key)
        .ok_or_else(|| format!("Missing or invalid field '{}': field not found", key))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| format!("Missing or invalid field '{}': {}", key, e))
        })
}

/// Extract an optional field. Returns `None` if the field is absent,
/// `Some(v)` if present and valid. Logs a warning if present but malformed
/// (type mismatch) to aid debugging — treats malformed as missing to avoid
/// breaking callers that rely on lenient behavior.
pub fn de_opt<T: serde::de::DeserializeOwned>(args: &serde_json::Value, key: &str) -> Option<T> {
    match get_field(args, key) {
        None => None,
        Some(v) => match serde_json::from_value::<T>(v.clone()) {
            Ok(val) => Some(val),
            Err(e) => {
                tracing::warn!(
                    field = key,
                    error = %e,
                    value = %v,
                    "Optional field present but malformed — treating as missing"
                );
                None
            }
        },
    }
}

/// Strict version of de_opt that returns an error instead of None on type mismatch.
/// Use this for optional fields where a wrong type should fail the command.
pub fn de_opt_strict<T: serde::de::DeserializeOwned>(
    args: &serde_json::Value,
    key: &str,
) -> Result<Option<T>, String> {
    match get_field(args, key) {
        None => Ok(None),
        Some(v) => serde_json::from_value::<T>(v.clone())
            .map(Some)
            .map_err(|e| {
                format!(
                    "Invalid field '{}': expected {}, got {}: {}",
                    key,
                    std::any::type_name::<T>(),
                    value_type_name(v),
                    e
                )
            }),
    }
}

pub fn value_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

pub fn to_json<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| format!("JSON serialization error: {}", e))
}

pub fn ok_null() -> Result<String, String> {
    Ok("null".to_string())
}
