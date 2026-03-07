//! Typed command dispatch — compile-time-checked command structs (PBI-234).
//!
//! Each domain module exposes a `dispatch_typed()` that matches typed commands,
//! returning `Some(result)` on match or `None` to fall through to the legacy
//! string-based handlers.

pub mod files_lifecycle;
pub mod folders;
pub mod selection;
pub mod tags;

use std::future::Future;

/// A command with typed input/output and compile-time-checked deserialization.
pub trait TypedCommand {
    const NAME: &'static str;
    type Input: serde::de::DeserializeOwned + Send;
    type Output: serde::Serialize + Send;

    fn execute(
        state: &crate::state::AppState,
        input: Self::Input,
    ) -> impl Future<Output = Result<Self::Output, String>> + Send;
}

/// Deserialize args, execute a typed command, serialize output.
async fn run_typed<C: TypedCommand>(
    state: &crate::state::AppState,
    args: &serde_json::Value,
) -> Result<String, String> {
    let input: C::Input = serde_json::from_value(args.clone()).map_err(|e| {
        format!("Invalid args for '{}': {}", C::NAME, e)
    })?;
    let output = C::execute(state, input).await?;
    super::common::to_json(&output)
}

/// Try typed dispatch. Returns `Some(result)` if a typed command matched,
/// `None` to fall through to legacy domain handlers.
pub async fn typed_dispatch(
    state: &crate::state::AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    if let Some(result) = files_lifecycle::dispatch_typed(state, command, args).await {
        return Some(result);
    }
    if let Some(result) = folders::dispatch_typed(state, command, args).await {
        return Some(result);
    }
    if let Some(result) = tags::dispatch_typed(state, command, args).await {
        return Some(result);
    }
    if let Some(result) = selection::dispatch_typed(state, command, args).await {
        return Some(result);
    }
    None
}
