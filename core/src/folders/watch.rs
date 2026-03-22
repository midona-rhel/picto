use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;

use crate::blob_store::BlobStore;
use crate::duplicates::orchestrator::DuplicateOrchestrator;
use crate::events::{self, ManualImportProgressEvent};
use crate::folders::service;
use crate::import::existing::{merge_existing_import_target, ExistingImportMergeRequest};
use crate::import::pipeline::{ImportError, ImportOptions, ImportPipeline};
use crate::media_derivatives;
use crate::runtime_contract::change_builder::ChangeImpact;
use crate::sqlite::SqliteDatabase;

const WATCH_DEBOUNCE: Duration = Duration::from_millis(700);
const WATCH_SWEEP_INTERVAL: Duration = Duration::from_millis(250);
const FILE_STABLE_WAIT: Duration = Duration::from_millis(250);
const FILE_STABLE_POLLS: usize = 6;

#[derive(Debug, Clone)]
pub enum FolderWatchCommand {
    Reload,
}

#[derive(Debug, Clone)]
struct WatchedFolderConfig {
    folder_id: i64,
    root_path: PathBuf,
    watch_subfolders: bool,
    watch_import_status_mode: String,
}

#[derive(Debug, Clone)]
struct RawWatchEvent {
    root_path: PathBuf,
    event: Event,
}

#[derive(Debug, Clone)]
struct PendingWatchPath {
    root_path: PathBuf,
    path: PathBuf,
    queued_at: Instant,
}

pub fn channel() -> (
    UnboundedSender<FolderWatchCommand>,
    UnboundedReceiver<FolderWatchCommand>,
) {
    tokio::sync::mpsc::unbounded_channel()
}

pub fn spawn_worker(
    db: Arc<SqliteDatabase>,
    blob_store: Arc<BlobStore>,
    mut command_rx: UnboundedReceiver<FolderWatchCommand>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<RawWatchEvent>();
        let mut runtime = FolderWatchRuntime::new(db, blob_store, event_tx);
        runtime.reload().await;
        let mut sweep = tokio::time::interval(WATCH_SWEEP_INTERVAL);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    tracing::info!("Folder watch worker cancelled");
                    return;
                }
                Some(command) = command_rx.recv() => {
                    match command {
                        FolderWatchCommand::Reload => runtime.reload().await,
                    }
                }
                Some(raw) = event_rx.recv() => {
                    runtime.enqueue(raw);
                }
                _ = sweep.tick() => {
                    runtime.flush_due().await;
                }
            }
        }
    })
}

pub async fn import_existing_for_folder_watch(
    db: &SqliteDatabase,
    blob_store: &BlobStore,
    folder_id: i64,
    watch_path: &str,
    watch_subfolders: bool,
    watch_import_status_mode: &str,
) -> Result<(), String> {
    let root_path = PathBuf::from(watch_path);
    if !root_path.is_dir() {
        return Err(format!("Watch folder not found: {}", root_path.display()));
    }

    let file_paths = collect_existing_paths(&root_path, watch_subfolders)?;
    if file_paths.is_empty() {
        return Ok(());
    }

    for (index, path) in file_paths.iter().enumerate() {
        process_import_path(
            db,
            blob_store,
            folder_id,
            &root_path,
            watch_subfolders,
            watch_import_status_mode,
            path,
        )
        .await?;

        emit_progress(
            index + 1,
            file_paths.len(),
            path.strip_prefix(&root_path)
                .unwrap_or(path)
                .display()
                .to_string(),
        );
    }

    Ok(())
}

struct FolderWatchRuntime {
    db: Arc<SqliteDatabase>,
    blob_store: Arc<BlobStore>,
    event_tx: UnboundedSender<RawWatchEvent>,
    configs: HashMap<PathBuf, WatchedFolderConfig>,
    watchers: HashMap<PathBuf, RecommendedWatcher>,
    pending: HashMap<PathBuf, PendingWatchPath>,
}

impl FolderWatchRuntime {
    fn new(
        db: Arc<SqliteDatabase>,
        blob_store: Arc<BlobStore>,
        event_tx: UnboundedSender<RawWatchEvent>,
    ) -> Self {
        Self {
            db,
            blob_store,
            event_tx,
            configs: HashMap::new(),
            watchers: HashMap::new(),
            pending: HashMap::new(),
        }
    }

    async fn reload(&mut self) {
        self.watchers.clear();
        self.configs.clear();
        self.pending.clear();

        let folders = match self.db.list_watched_folders().await {
            Ok(folders) => folders,
            Err(err) => {
                tracing::warn!(error = %err, "Failed to load watched folder configs");
                return;
            }
        };

        for folder in folders {
            let Some(watch_path) = folder.watch_path.clone() else {
                continue;
            };
            let root_path = PathBuf::from(&watch_path);
            if !root_path.is_dir() {
                tracing::warn!(path = %root_path.display(), folder_id = folder.folder_id, "Skipping missing watched folder path");
                continue;
            }
            let recursive_mode = if folder.watch_subfolders {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            let tx = self.event_tx.clone();
            let root_for_cb = root_path.clone();
            let mut watcher = match notify::recommended_watcher(move |result| match result {
                Ok(event) => {
                    let _ = tx.send(RawWatchEvent {
                        root_path: root_for_cb.clone(),
                        event,
                    });
                }
                Err(err) => {
                    tracing::warn!(error = %err, path = %root_for_cb.display(), "Folder watch event error");
                }
            }) {
                Ok(watcher) => watcher,
                Err(err) => {
                    tracing::warn!(error = %err, path = %root_path.display(), "Failed to create folder watcher");
                    continue;
                }
            };
            if let Err(err) = watcher.watch(&root_path, recursive_mode) {
                tracing::warn!(error = %err, path = %root_path.display(), "Failed to watch folder path");
                continue;
            }

            self.configs.insert(
                root_path.clone(),
                WatchedFolderConfig {
                    folder_id: folder.folder_id,
                    root_path: root_path.clone(),
                    watch_subfolders: folder.watch_subfolders,
                    watch_import_status_mode: folder.watch_import_status_mode,
                },
            );
            self.watchers.insert(root_path, watcher);
        }

        tracing::info!(
            folders = self.configs.len(),
            "folder watch: reload complete"
        );
    }

    fn enqueue(&mut self, raw: RawWatchEvent) {
        if !should_handle_event(&raw.event.kind) {
            return;
        }
        for path in &raw.event.paths {
            if should_ignore_path(path) || !crate::media_processing::has_supported_extension(path) {
                continue;
            }
            self.pending.insert(
                path.clone(),
                PendingWatchPath {
                    root_path: raw.root_path.clone(),
                    path: path.clone(),
                    queued_at: Instant::now(),
                },
            );
        }
    }

    async fn flush_due(&mut self) {
        let now = Instant::now();
        let mut due = Vec::new();
        self.pending.retain(|_, pending| {
            if now.duration_since(pending.queued_at) >= WATCH_DEBOUNCE {
                due.push(pending.clone());
                false
            } else {
                true
            }
        });

        for pending in due {
            let Some(config) = self.configs.get(&pending.root_path).cloned() else {
                continue;
            };
            if !wait_for_file_stable(&pending.path).await {
                continue;
            }
            match process_import_path(
                &self.db,
                &self.blob_store,
                config.folder_id,
                &config.root_path,
                config.watch_subfolders,
                &config.watch_import_status_mode,
                &pending.path,
            )
            .await
            {
                Ok(()) => {
                    tracing::info!(path = %pending.path.display(), folder_id = config.folder_id, "folder watch: file imported");
                }
                Err(err) => {
                    tracing::warn!(error = %err, path = %pending.path.display(), "folder watch: import failed");
                }
            }
        }
    }
}

fn should_handle_event(kind: &EventKind) -> bool {
    matches!(kind, EventKind::Create(_) | EventKind::Modify(_))
}

fn should_ignore_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return true;
    };
    if name.starts_with('.') {
        return true;
    }
    [".part", ".crdownload", ".tmp", ".download", ".ds_store"]
        .iter()
        .any(|suffix| name.to_ascii_lowercase().ends_with(suffix))
}

async fn wait_for_file_stable(path: &Path) -> bool {
    let mut last_len = None;
    let mut stable_hits = 0usize;

    for _ in 0..FILE_STABLE_POLLS {
        match tokio::fs::metadata(path).await {
            Ok(metadata) if metadata.is_file() => {
                let len = metadata.len();
                if Some(len) == last_len {
                    stable_hits += 1;
                    if stable_hits >= 2 {
                        return true;
                    }
                } else {
                    stable_hits = 0;
                    last_len = Some(len);
                }
            }
            _ => return false,
        }
        tokio::time::sleep(FILE_STABLE_WAIT).await;
    }

    false
}

fn collect_existing_paths(root_path: &Path, recursive: bool) -> Result<Vec<PathBuf>, String> {
    if !recursive {
        let mut files = Vec::new();
        let entries = fs::read_dir(root_path)
            .map_err(|err| format!("Failed to read {}: {err}", root_path.display()))?;
        for entry in entries {
            let entry = entry
                .map_err(|err| format!("Failed to read entry in {}: {err}", root_path.display()))?;
            let path = entry.path();
            if path.is_file()
                && !should_ignore_path(&path)
                && crate::media_processing::has_supported_extension(&path)
            {
                files.push(path);
            }
        }
        files.sort();
        return Ok(files);
    }

    let mut files = Vec::new();
    let mut stack = vec![root_path.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|err| format!("Failed to read {}: {err}", directory.display()))?;
        let mut children = Vec::new();
        for entry in entries {
            let entry = entry
                .map_err(|err| format!("Failed to read entry in {}: {err}", directory.display()))?;
            children.push(entry.path());
        }
        children.sort();
        for child in children {
            if should_ignore_path(&child) {
                continue;
            }
            if child.is_dir() {
                stack.push(child);
            } else if child.is_file() && crate::media_processing::has_supported_extension(&child) {
                files.push(child);
            }
        }
    }
    files.sort();
    Ok(files)
}

async fn process_import_path(
    db: &SqliteDatabase,
    blob_store: &BlobStore,
    folder_id: i64,
    root_path: &Path,
    watch_subfolders: bool,
    watch_import_status_mode: &str,
    path: &Path,
) -> Result<(), String> {
    let relative_parent = path
        .strip_prefix(root_path)
        .ok()
        .and_then(|relative| relative.parent())
        .unwrap_or_else(|| Path::new(""));
    if !watch_subfolders && !relative_parent.as_os_str().is_empty() {
        return Ok(());
    }

    let target_folder_id = if relative_parent.as_os_str().is_empty() {
        folder_id
    } else {
        ensure_relative_folder_path(db, folder_id, relative_parent).await?
    };

    let initial_status = resolve_initial_status(watch_import_status_mode)?;
    import_file_into_folder(db, blob_store, path, target_folder_id, initial_status).await
}

async fn ensure_relative_folder_path(
    db: &SqliteDatabase,
    root_folder_id: i64,
    relative_parent: &Path,
) -> Result<i64, String> {
    let mut current_folder_id = root_folder_id;
    for component in relative_parent.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let Some(name) = name.to_str() else {
            continue;
        };
        let child = service::ensure_child_folder(db, current_folder_id, name).await?;
        current_folder_id = child.folder_id;
    }
    Ok(current_folder_id)
}

fn resolve_initial_status(mode: &str) -> Result<i64, String> {
    match mode {
        "inbox" => Ok(0),
        "active" => Ok(1),
        "inherit" => {
            let default_mode = crate::state::get_state()
                .map(|state| state.settings.get().watch_folder_default_status)
                .unwrap_or_else(|_| "inbox".to_string());
            resolve_initial_status(&default_mode)
        }
        other => Err(format!("Invalid watch import status mode: {other}")),
    }
}

async fn import_file_into_folder(
    db: &SqliteDatabase,
    blob_store: &BlobStore,
    path: &Path,
    folder_id: i64,
    initial_status: i64,
) -> Result<(), String> {
    let app_settings = crate::state::get_state()
        .map(|state| state.settings.get())
        .unwrap_or_default();
    let auto_merge_enabled = app_settings.duplicate_auto_merge_enabled
        && !app_settings.duplicate_auto_merge_subscriptions_only;
    let auto_merge_distance = if auto_merge_enabled {
        crate::settings::store::similarity_pct_to_distance(
            app_settings.duplicate_auto_merge_similarity_pct,
        )
    } else {
        0
    };
    let auto_merge_require_matching_dimensions =
        app_settings.duplicate_auto_merge_require_matching_dimensions;

    let pipeline = ImportPipeline::new(db, blob_store);
    let options = ImportOptions {
        initial_status,
        ..ImportOptions::default()
    };

    let mut imported_hashes = Vec::<String>::new();
    let mut skipped_hashes = Vec::<String>::new();

    match pipeline.import_file(path, &options).await {
        Ok(imported) => {
            let surviving_hash = maybe_auto_merge(
                db,
                blob_store,
                &imported.hex_hash,
                auto_merge_enabled,
                auto_merge_distance,
                auto_merge_require_matching_dimensions,
            )
            .await;
            if surviving_hash == imported.hex_hash {
                emit_file_imported(db, &surviving_hash).await;
            }
            media_derivatives::enqueue_import_derivatives(
                db,
                &surviving_hash,
                &imported.mime,
                options.skip_thumbnail,
            )
            .await?;
            imported_hashes.push(surviving_hash);
        }
        Err(ImportError::AlreadyImported(hash)) => {
            merge_existing_import_target(
                db,
                &hash,
                ExistingImportMergeRequest {
                    restore_status: Some(initial_status),
                    tag_strings: Vec::new(),
                    source_urls: Vec::new(),
                    created_at: None,
                    name: None,
                    note_entries: Default::default(),
                    subscription_id: None,
                    change_origin: "watch_folder_existing",
                },
            )
            .await?;
            skipped_hashes.push(hash);
        }
        Err(err) => return Err(err.to_string()),
    }

    let membership_hashes: Vec<String> = imported_hashes
        .iter()
        .cloned()
        .chain(skipped_hashes.iter().cloned())
        .collect();
    if !membership_hashes.is_empty() {
        db.add_entities_to_folder_batch(folder_id, &membership_hashes)
            .await?;
        service::refresh_sidebar_projection_for_folder_ids(db, &[folder_id]).await?;
    }

    let mut impact: Option<ChangeImpact> = None;
    if !imported_hashes.is_empty() {
        let next = ChangeImpact::file_lifecycle(db)
            .file_hashes(imported_hashes.clone())
            .merge(ChangeImpact::folder_file_change(folder_id));
        impact = Some(match impact.take() {
            Some(current) => current.merge(next),
            None => next,
        });
    }
    if !skipped_hashes.is_empty() {
        let next = ChangeImpact::new()
            .file_hashes(skipped_hashes.clone())
            .merge(ChangeImpact::folder_file_change(folder_id));
        impact = Some(match impact.take() {
            Some(current) => current.merge(next),
            None => next,
        });
    }

    if let Some(impact) = impact {
        let origin = if !imported_hashes.is_empty() {
            "watch_folder_import"
        } else {
            "watch_folder_membership"
        };
        crate::events::emit_state_changed(origin, impact);
    }

    Ok(())
}

async fn maybe_auto_merge(
    db: &SqliteDatabase,
    blob_store: &BlobStore,
    hash: &str,
    auto_merge_enabled: bool,
    auto_merge_distance: u32,
    auto_merge_require_matching_dimensions: bool,
) -> String {
    if !auto_merge_enabled {
        return hash.to_string();
    }
    match DuplicateOrchestrator::check_and_auto_merge(
        db,
        blob_store,
        hash,
        auto_merge_distance,
        auto_merge_require_matching_dimensions,
    )
    .await
    {
        Ok(Some(result)) => result.winner_hash,
        Ok(None) => hash.to_string(),
        Err(err) => {
            tracing::warn!(error = %err, hash = %hash, "Folder watch auto-merge failed");
            hash.to_string()
        }
    }
}

async fn emit_file_imported(db: &SqliteDatabase, hash: &str) {
    if let Ok(Some(record)) = db.get_file_by_hash(hash).await {
        let slim = crate::types::FileInfoSlim::from(record);
        crate::events::emit(crate::events::event_names::FILE_IMPORTED, &slim);
    }
}

fn emit_progress(done: usize, total: usize, current_file: String) {
    events::emit(
        events::event_names::MANUAL_IMPORT_PROGRESS,
        &ManualImportProgressEvent {
            done,
            total,
            current_file,
            imported: done,
            skipped: 0,
            errors: 0,
        },
    );
}
