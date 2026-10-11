//! Renderer-to-core IPC argument contract.
//!
//! `tests/fixtures/ipc_payloads.json` is recorded by
//! `src/platform/ipcContract.test.ts`: every `[command, args]` payload the
//! platform API layer sends. Each one goes through the real command dispatcher
//! here, so a field the renderer renames or reshapes (or one Rust starts
//! requiring) fails as `Invalid command arguments` instead of in the app.
//! Regenerate the fixture with `npx vitest run src/platform/ipcContract.test.ts -u`.

use std::path::Path;
use std::sync::Arc;

use picto_core::library_application::LibraryApplication;
use serde_json::Value;

const FIXTURE: &str = "tests/fixtures/ipc_payloads.json";

/// Paths in the fixture start here; they are rebased into the temp directory
/// so exports, packs and imports never touch the real filesystem.
const PATH_PREFIX: &str = "/picto-contract";

/// Prefix produced only by `ipc::parse`, i.e. a payload shape mismatch.
const PARSE_ERROR: &str = "Invalid command arguments:";

/// Terminated by the Electron host (`electron/ipc/registerHandlers.mjs`) and
/// never forwarded to core. Their payloads are not checked here.
const HOST_COMMANDS: &[&str] = &[
    "auth_onlyfans_manual",
    "auth_session_cancel",
    "auth_session_start",
    "auth_session_state",
    "media.set_thumbnail",
    "open_external_url",
    "open_in_new_window",
    "open_settings_window",
];

/// Commands with effects outside the temp library. Keep this list minimal.
const SKIPPED: &[(&str, &str)] = &[("ai.models.download", "downloads a model over the network")];

fn load_fixture() -> Vec<(String, Value)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

fn rebase_paths(value: &mut Value, root: &Path) {
    match value {
        Value::String(text) => {
            if let Some(rest) = text.strip_prefix(PATH_PREFIX) {
                let rebased = root.join(rest.trim_start_matches('/'));
                *text = rebased.to_string_lossy().into_owned();
            }
        }
        Value::Array(values) => values.iter_mut().for_each(|value| rebase_paths(value, root)),
        Value::Object(map) => map.values_mut().for_each(|value| rebase_paths(value, root)),
        _ => {}
    }
}

/// The library lives one level down: AI model storage falls back to the
/// library's parent directory, which must also be inside the temp dir.
fn open_library(directory: &Path) -> Arc<LibraryApplication> {
    // Credential commands must never read or write the OS keychain.
    std::env::set_var("PICTO_EPHEMERAL_CREDENTIALS", "1");
    let root = directory.join("library");
    std::fs::create_dir_all(&root).unwrap();
    Arc::new(LibraryApplication::create(&root).unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_recorded_renderer_payload_parses_in_core() {
    let fixture = load_fixture();
    assert!(!fixture.is_empty(), "{FIXTURE} has no payloads");
    let directory = tempfile::tempdir().unwrap();
    let application = open_library(directory.path());
    let files = directory.path().join("files");
    std::fs::create_dir_all(&files).unwrap();

    let mut failures = Vec::new();
    for (command, mut args) in fixture {
        if SKIPPED.iter().any(|(skipped, _)| *skipped == command) {
            continue;
        }
        rebase_paths(&mut args, &files);
        let args_json = serde_json::to_string(&args).unwrap();
        let result =
            picto_core::ipc::dispatch_library_async(&application, &command, &args_json).await;
        let host_command = HOST_COMMANDS.contains(&command.as_str());
        match result {
            Ok(None) if host_command => {}
            Ok(None) => failures.push(format!("{command}: not dispatched by core")),
            Ok(Some(_)) if host_command => failures.push(format!(
                "{command}: now handled by core; remove it from HOST_COMMANDS"
            )),
            Ok(Some(_)) => {}
            Err(error) if error.starts_with(PARSE_ERROR) => {
                failures.push(format!("{command}: {error}\n    args: {args_json}"))
            }
            // A domain error (missing folder, unknown model, ...) means the
            // payload parsed; the shape is all this test checks.
            Err(_) => {}
        }
    }
    assert!(
        failures.is_empty(),
        "renderer payloads rejected by core (fix the side that changed, then \
         regenerate {FIXTURE}):\n  {}",
        failures.join("\n  ")
    );
}

/// Clearing notes is the one patch where `null` carries meaning; a payload
/// that parses but leaves the notes in place is the same contract bug.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recorded_notes_clear_payload_clears_notes() {
    let mut clear = load_fixture()
        .into_iter()
        .find(|(command, args)| {
            command == "items.patch_metadata"
                && args.pointer("/patch/notes") == Some(&Value::Null)
        })
        .map(|(_, args)| args)
        .expect("fixture records an items.patch_metadata payload that clears notes");

    let directory = tempfile::tempdir().unwrap();
    let application = open_library(directory.path());
    let (root_id, _) = application
        .library()
        .ingest(&picto_library::PreparedImport {
            stable_key: "contract-root".into(),
            media_name: "Contract item".into(),
            file_path: directory.path().join("contract.png").to_string_lossy().into(),
            facts: picto_library::ImmutableMediaFacts {
                mime: "image/png".into(),
                size_bytes: 12,
                width: Some(10),
                height: Some(20),
                duration_ms: None,
                frame_count: Some(1),
                content_hash: "contract-hash".into(),
                perceptual_hash: None,
                palette: Vec::new(),
            },
            lifecycle: picto_library::Lifecycle::Active,
            rating: picto_library::Rating::Unrated,
            notes: Some("Before".into()),
            tags: Vec::new(),
            folders: Vec::new(),
            source_urls: Vec::new(),
            imported_at_ms: 1_700_000_000_000,
            captured_at_ms: None,
            source_identity: None,
        })
        .unwrap();
    assert_eq!(
        application.details(root_id).unwrap().root.notes.as_deref(),
        Some("Before")
    );

    clear["target"] = serde_json::json!({"kind": "explicit", "root_ids": [root_id]});
    picto_core::ipc::dispatch_library_async(
        &application,
        "items.patch_metadata",
        &clear.to_string(),
    )
    .await
    .unwrap()
    .expect("items.patch_metadata is a core command");
    assert_eq!(application.details(root_id).unwrap().root.notes, None);
}
