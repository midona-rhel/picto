use napi::bindgen_prelude::*;
use napi::threadsafe_function::{
    ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
};
use napi_derive::napi;
#[cfg(target_os = "macos")]
use std::ffi::{CStr, CString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

// ── macOS native drag (bypasses Electron's startDrag to avoid icon stacking) ──

#[cfg(target_os = "macos")]
extern "C" {
    fn picto_start_file_drag(
        ns_view_ptr: *mut std::ffi::c_void,
        paths: *const *const std::ffi::c_char,
        path_count: std::ffi::c_int,
        rgba_data: *const u8,
        icon_width: std::ffi::c_int,
        icon_height: std::ffi::c_int,
    );
    fn picto_get_associated_applications(
        file_path: *const std::ffi::c_char,
    ) -> *const std::ffi::c_char;
    fn picto_free_string(value: *const std::ffi::c_char);
    fn picto_open_with_application(
        application_path: *const std::ffi::c_char,
        file_path: *const std::ffi::c_char,
    ) -> bool;
    fn picto_set_file_icon(
        icon_path: *const std::ffi::c_char,
        file_path: *const std::ffi::c_char,
    ) -> bool;
}

/// Put physical files on the operating-system clipboard.
#[napi]
pub fn copy_files(file_paths: Vec<String>) -> Result<bool> {
    if file_paths.is_empty() {
        return Ok(false);
    }

    #[cfg(target_os = "windows")]
    {
        use std::mem::size_of;
        use std::ptr::{copy_nonoverlapping, null_mut};
        use windows_sys::Win32::System::DataExchange::{
            CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData, CF_HDROP,
        };
        use windows_sys::Win32::System::Memory::{
            GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock, GHND,
        };
        use windows_sys::Win32::UI::Shell::DROPFILES;

        let mut names = Vec::<u16>::new();
        for path in &file_paths {
            names.extend(path.encode_utf16());
            names.push(0);
        }
        names.push(0);
        let allocation_size = size_of::<DROPFILES>() + names.len() * size_of::<u16>();

        unsafe {
            if OpenClipboard(null_mut()) == 0 {
                return Err(Error::from_reason("Could not open the system clipboard"));
            }
            EmptyClipboard();
            let memory = GlobalAlloc(GHND, allocation_size);
            if memory.is_null() {
                CloseClipboard();
                return Err(Error::from_reason(
                    "Could not allocate a file clipboard payload",
                ));
            }
            let locked = GlobalLock(memory) as *mut u8;
            if locked.is_null() {
                GlobalFree(memory);
                CloseClipboard();
                return Err(Error::from_reason(
                    "Could not prepare a file clipboard payload",
                ));
            }
            let header = locked as *mut DROPFILES;
            (*header).pFiles = size_of::<DROPFILES>() as u32;
            (*header).fWide = 1;
            copy_nonoverlapping(
                names.as_ptr() as *const u8,
                locked.add(size_of::<DROPFILES>()),
                names.len() * size_of::<u16>(),
            );
            GlobalUnlock(memory);
            if SetClipboardData(CF_HDROP as u32, memory).is_null() {
                GlobalFree(memory);
                CloseClipboard();
                return Err(Error::from_reason(
                    "Could not write files to the system clipboard",
                ));
            }
            CloseClipboard();
        }
        return Ok(true);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = file_paths;
        Ok(false)
    }
}

#[napi]
pub async fn get_associated_applications(file_path: String) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let path = CString::new(file_path).map_err(|_| Error::from_reason("Invalid file path"))?;
        let value = unsafe { picto_get_associated_applications(path.as_ptr()) };
        if value.is_null() {
            return Err(Error::from_reason(
                "Could not resolve associated applications",
            ));
        }
        let result = unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned();
        unsafe { picto_free_string(value) };
        return Ok(result);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = file_path;
        Ok("[]".to_string())
    }
}

#[napi]
pub fn open_with_application(application_path: String, file_path: String) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let application = CString::new(application_path)
            .map_err(|_| Error::from_reason("Invalid application path"))?;
        let file = CString::new(file_path).map_err(|_| Error::from_reason("Invalid file path"))?;
        if !unsafe { picto_open_with_application(application.as_ptr(), file.as_ptr()) } {
            return Err(Error::from_reason(
                "Could not open file with the selected application",
            ));
        }
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (application_path, file_path);
        Err(Error::from_reason(
            "Application selection is not supported on this platform yet",
        ))
    }
}

/// Apply a persistent custom Finder icon to a file or package on macOS.
#[napi]
pub fn set_file_icon(icon_path: String, file_path: String) -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        let icon = CString::new(icon_path).map_err(|_| Error::from_reason("Invalid icon path"))?;
        let file = CString::new(file_path).map_err(|_| Error::from_reason("Invalid file path"))?;
        return Ok(unsafe { picto_set_file_icon(icon.as_ptr(), file.as_ptr()) });
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (icon_path, file_path);
        Ok(false)
    }
}

/// Start a native file drag on macOS with a single composite icon.
/// On non-macOS platforms this is a no-op (use Electron's startDrag instead).
#[napi]
pub fn start_native_drag(
    window_handle: Buffer,
    file_paths: Vec<String>,
    icon_rgba: Buffer,
    icon_width: i32,
    icon_height: i32,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;

        let handle_bytes = window_handle.as_ref();
        if handle_bytes.len() < std::mem::size_of::<*mut std::ffi::c_void>() {
            return Err(Error::from_reason("Invalid window handle buffer"));
        }
        let view_ptr = unsafe { *(handle_bytes.as_ptr() as *const *mut std::ffi::c_void) };

        let c_paths: Vec<CString> = file_paths
            .iter()
            .filter_map(|p| CString::new(p.as_str()).ok())
            .collect();
        let c_ptrs: Vec<*const std::ffi::c_char> = c_paths.iter().map(|p| p.as_ptr()).collect();

        unsafe {
            picto_start_file_drag(
                view_ptr,
                c_ptrs.as_ptr(),
                c_ptrs.len() as std::ffi::c_int,
                icon_rgba.as_ref().as_ptr(),
                icon_width,
                icon_height,
            );
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (
            window_handle,
            file_paths,
            icon_rgba,
            icon_width,
            icon_height,
        );
    }

    Ok(())
}

static EVENTS_DROPPED: AtomicU64 = AtomicU64::new(0);

/// Newtype wrapper for event data to use with ThreadsafeFunction.
struct EventData {
    name: String,
    payload_json: String,
}

static EVENT_CB: OnceLock<Mutex<ThreadsafeFunction<EventData>>> = OnceLock::new();

/// Initialize tracing and runtime. Called once at process startup.
#[napi]
pub fn init_runtime(application_data_root: String) -> Result<()> {
    picto_core::state_v2::set_application_data_root(PathBuf::from(application_data_root))
        .map_err(Error::from_reason)?;
    picto_core::state_v2::init_tracing();
    Ok(())
}

#[napi]
pub async fn healthcheck() -> String {
    "ok".to_string()
}

/// Open a library at the given path. Closes any previously open library first.
#[napi]
pub async fn open_library(library_path: String) -> Result<()> {
    let path = PathBuf::from(library_path);
    picto_core::state_v2::open_library(path)
        .await
        .map_err(|e| Error::from_reason(e))?;
    Ok(())
}

/// Open an isolated tutorial library backed only by bundled local fixtures.
#[napi]
pub async fn open_tutorial_library(library_path: String, fixture_root: String) -> Result<()> {
    picto_core::state_v2::open_tutorial_library(
        PathBuf::from(library_path),
        PathBuf::from(fixture_root),
    )
    .await
    .map_err(Error::from_reason)?;
    Ok(())
}

/// Close the currently open library, stopping all background tasks.
#[napi]
pub async fn close_library() -> Result<()> {
    picto_core::state_v2::close_library()
        .await
        .map_err(|e| Error::from_reason(e))?;
    Ok(())
}

/// Dispatch a command to the core engine.
/// `command` is the command name, `args_json` is a JSON-encoded arguments object.
/// Returns a JSON-encoded result string.
#[napi]
pub async fn invoke(command: String, args_json: String) -> Result<String> {
    picto_core::state_v2::invoke(&command, &args_json)
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
