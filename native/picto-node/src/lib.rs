use napi::bindgen_prelude::*;
use napi::threadsafe_function::{
    ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
};
use napi_derive::napi;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

static EVENTS_DROPPED: AtomicU64 = AtomicU64::new(0);

/// Newtype wrapper for event data to use with ThreadsafeFunction.
struct EventData {
    name: String,
    payload_json: String,
}

static EVENT_CB: OnceLock<Mutex<ThreadsafeFunction<EventData>>> = OnceLock::new();

#[napi]
pub async fn healthcheck() -> String {
    "ok".to_string()
}

/// Open a library at the given path. Closes any previously open library first.
#[napi]
pub async fn open_library(library_path: String) -> Result<()> {
    let path = PathBuf::from(library_path);
    picto_core::state::open_library(path)
        .await
        .map_err(|e| Error::from_reason(e))?;
    Ok(())
}

/// Backward-compatible alias for `open_library`.
#[napi]
pub async fn initialize(library_path: String) -> Result<()> {
    open_library(library_path).await
}

/// Close the currently open library, stopping all background tasks.
#[napi]
pub async fn close_library() -> Result<()> {
    picto_core::state::close_library()
        .await
        .map_err(|e| Error::from_reason(e))?;
    Ok(())
}

/// Dispatch a command to the core engine.
/// `command` is the command name, `args_json` is a JSON-encoded arguments object.
/// Returns a JSON-encoded result string.
#[napi]
pub async fn invoke(command: String, args_json: String) -> Result<String> {
    picto_core::dispatch::dispatch(&command, &args_json)
        .await
        .map_err(|e| Error::from_reason(e))
}

/// Register a callback that receives native events from the core engine.
/// The callback receives (event_name: string, payload_json: string).
#[napi]
pub fn register_event_callback(callback: JsFunction) -> Result<()> {
    let tsfn: ThreadsafeFunction<EventData> =
        callback.create_threadsafe_function(0, |ctx: ThreadSafeCallContext<EventData>| {
            Ok(vec![
                ctx.env.create_string(&ctx.value.name)?.into_unknown(),
                ctx.env
                    .create_string(&ctx.value.payload_json)?
                    .into_unknown(),
            ])
        })?;

    // Store in the napi-side slot for emit_test_event
    let slot = EVENT_CB.get_or_init(|| Mutex::new(tsfn.clone()));
    let mut guard = slot
        .lock()
        .map_err(|_| Error::from_reason("event callback lock poisoned"))?;
    *guard = tsfn.clone();

    // Wire into the core event system so core can emit events to Electron
    let core_tsfn = tsfn;
    picto_core::events::set_event_callback(move |name, payload_json| {
        let status = core_tsfn.call(
            Ok(EventData {
                name: name.to_string(),
                payload_json: payload_json.to_string(),
            }),
            ThreadsafeFunctionCallMode::NonBlocking,
        );
        if status != napi::Status::Ok {
            let count = EVENTS_DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
            if count == 1 || count % 100 == 0 {
                eprintln!(
                    "[picto-node] event delivery failed (status={:?}, total_dropped={})",
                    status, count
                );
            }
        }
    });

    Ok(())
}

#[napi]
pub fn emit_test_event(name: String, payload_json: String) -> Result<()> {
    if let Some(slot) = EVENT_CB.get() {
        let guard = slot
            .lock()
            .map_err(|_| Error::from_reason("event callback lock poisoned"))?;
        let status = guard.call(
            Ok(EventData { name, payload_json }),
            ThreadsafeFunctionCallMode::NonBlocking,
        );
        if status != napi::Status::Ok {
            EVENTS_DROPPED.fetch_add(1, Ordering::Relaxed);
        }
    }
    Ok(())
}
