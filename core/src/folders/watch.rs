use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;

use crate::blob_store::BlobStore;
use crate::db::LibraryDatabase;
use crate::events::{self, ManualImportProgressEvent};
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
    canonical_db: Arc<LibraryDatabase>,
    blob_store: Arc<BlobStore>,
    mut command_rx: UnboundedReceiver<FolderWatchCommand>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<RawWatchEvent>();
        let mut runtime = FolderWatchRuntime::new(db, canonical_db, blob_store, event_tx);
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
    canonical_db: &LibraryDatabase,
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
            canonical_db,
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
    canonical_db: Arc<LibraryDatabase>,
    blob_store: Arc<BlobStore>,
    event_tx: UnboundedSender<RawWatchEvent>,
    configs: HashMap<PathBuf, WatchedFolderConfig>,
    watchers: HashMap<PathBuf, RecommendedWatcher>,
    pending: HashMap<PathBuf, PendingWatchPath>,
}

impl FolderWatchRuntime {
    fn new(
        db: Arc<SqliteDatabase>,
        canonical_db: Arc<LibraryDatabase>,
        blob_store: Arc<BlobStore>,
        event_tx: UnboundedSender<RawWatchEvent>,
    ) -> Self {
        Self {
            db,
            canonical_db,
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

        let folders = match self.canonical_db.list_folders_canonical() {
            Ok(folders) => folders,
            Err(err) => {
                tracing::warn!(error = %err, "Failed to load watched folder configs");
                return;
            }
        };

        for folder in folders {
            if !folder.watch_enabled {
                continue;
            }
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
                    watch_import_status_mode: folder
                        .watch_import_status_mode
                        .clone()
                        .unwrap_or_else(|| "inherit".to_string()),
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
                &self.canonical_db,
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
    canonical_db: &LibraryDatabase,
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

    let summary = crate::ingest::import_watch_path(
        canonical_db,
        Some(db),
        blob_store,
        folder_id,
        root_path,
        watch_subfolders,
        watch_import_status_mode,
        path,
    )
    .await?;

    crate::ingest::apply_compiler_plan(canonical_db, &summary.flags, &summary.folder_ids);
    if !summary.imported_hashes.is_empty() || !summary.skipped_hashes.is_empty() {
        let origin = if !summary.imported_hashes.is_empty() {
            "watch_folder_import"
        } else {
            "watch_folder_membership"
        };
        crate::events::emit_state_changed(
            origin,
            crate::ingest::build_ingest_change_impact(
                &summary,
                vec!["system:active".into(), "system:inbox".into()],
            ),
        );
    }

    Ok(())
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
