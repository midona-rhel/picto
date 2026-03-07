//! Shared dispatch utilities — JSON helpers, argument parsing, convenience wrappers.

/// Convert snake_case to camelCase: "folder_id" → "folderId"
fn snake_to_camel(s: &str) -> String {
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

pub fn to_json<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| format!("JSON serialization error: {}", e))
}

pub fn ok_null() -> Result<String, String> {
    Ok("null".to_string())
}
