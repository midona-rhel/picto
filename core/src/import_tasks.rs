//! Manual imports as foreground tasks.
//!
//! Starting an import only records a task; nothing is read on the caller's
//! thread. The scan worker walks the task's paths and queues one job per file
//! in batches, without reading the files, so the whole selection is recorded
//! quickly and survives a quit. The ingest worker then reads, hashes and
//! probes a few files ahead of each ingest batch (`prepare_task_jobs`) and
//! imports them as usual. A paused task's files stay queued; a stopped one's
//! are dropped; an undone one's created items are removed (`settle_reverts`).

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use picto_library::{
    FolderId, PendingCollectionImport, PendingImport, PreparedCollectionImport, PreparedImport,
    PreparedIngestJob, PreparedIngestPayload, TaskId, TaskIngestJob, TaskRecord, TaskState,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::library_application::LibraryApplication;
use crate::library_import::{self as import, ImportFields, ManualImportInput};

pub const IMPORT_TASK_KIND: &str = "import";
/// Files queued per transaction while scanning.
const SCAN_CHUNK: usize = 256;
/// Files read ahead of the ingest batches, and how many at once.
const PREPARE_AHEAD: usize = 48;
const PREPARE_CONCURRENCY: usize = 4;
/// How often a paused scan checks whether it may continue.
const PAUSED_SCAN_POLL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ImportStarted {
    #[ts(type = "number")]
    pub task_id: i64,
}

type FolderCache = BTreeMap<(Option<u32>, String), FolderId>;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Check the request and record it as a task. The scan worker does the rest.
pub fn start_manual_import(
    application: &LibraryApplication,
    input: &ManualImportInput,
) -> Result<ImportStarted, String> {
    if input.paths.is_empty() {
        return Err("At least one import path is required".into());
    }
    if input.watch_source_folder {
        if !input.preserve_structure {
            return Err(
                "Watching an imported folder requires preserving its folder structure".into(),
            );
        }
        if input.paths.len() != 1 {
            return Err("Watching an imported folder requires exactly one source folder".into());
        }
    }
    let library = fs::canonicalize(application.root())
        .map_err(|error| format!("Failed to resolve library path: {error}"))?;
    let mut resolved = Vec::with_capacity(input.paths.len());
    for value in &input.paths {
        let path = fs::canonicalize(value)
            .map_err(|error| format!("Failed to resolve import path '{value}': {error}"))?;
        if path.starts_with(&library) {
            return Err(format!(
                "Import path must be outside the library: {}",
                path.display()
            ));
        }
        resolved.push(path);
    }
    // Seeding (a background import) is not the user's; it never waits.
    if !input.background {
        crate::tasks::ensure_can_start(application, crate::tasks::IMPORT_ACTION)?;
    }
    let input_json = serde_json::to_string(input).map_err(|error| error.to_string())?;
    let task = application
        .library()
        .create_task(
            IMPORT_TASK_KIND,
            &import_title(&resolved),
            &input_json,
            // Moved files exist only in the library; undo would delete them.
            !input.delete_after_ingest,
            input.background,
            now_ms(),
        )
        .map_err(|error| error.to_string())?;
    Ok(ImportStarted { task_id: task.0 })
}

fn import_title(paths: &[PathBuf]) -> String {
    let name = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
    };
    match paths {
        [one] => name(one),
        [first, ..] => first.parent().and_then(name),
        [] => None,
    }
    .unwrap_or_else(|| "Import".into())
}

/// Walk the task's paths and queue every file found. After an interruption
/// it runs again from the start; files already queued keep their jobs.
/// Returns early, leaving the scan to resume later, when `cancelled` (the
/// library is closing); returns for good once the task is stopped.
pub fn scan_import_task(
    application: &LibraryApplication,
    task: &TaskRecord,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let library = application.library();
    let task_id = task.task_id;
    let input: ManualImportInput =
        serde_json::from_str(&task.input_json).map_err(|error| error.to_string())?;
    let fail = |error: &str| {
        library
            .fail_task(task_id, error, now_ms())
            .map_err(|failure| failure.to_string())
    };
    let candidates = match import::collect_manual_candidates(application.root(), &input) {
        Ok(candidates) => candidates,
        Err(error) => return fail(&error),
    };
    let structure_only = input.preserve_structure && input.include_folders_without_media;
    if candidates.is_empty() && !structure_only {
        return fail("No supported media files were found in the selected import.");
    }

    let mut folders = FolderCache::new();
    let capacity = if input.preserve_structure {
        library
            .folder_child_capacity(input.parent_folder_id)
            .map_err(|error| error.to_string())?
    } else {
        0
    };
    if structure_only {
        for value in &input.paths {
            let path = fs::canonicalize(value)
                .map_err(|error| format!("Failed to resolve import path '{value}': {error}"))?;
            if !path.is_dir() {
                continue;
            }
            for relative in import::collect_structure_directories(&path, input.include_subfolders)? {
                import::ensure_relative_folder(
                    application,
                    input.parent_folder_id,
                    import::truncate_relative_folder(&relative, capacity).as_deref(),
                    &mut folders,
                )?;
            }
        }
    }

    let mut chunk = Vec::with_capacity(SCAN_CHUNK);
    let mut members = Vec::new();
    for candidate in candidates {
        let path = candidate.path.to_string_lossy().into_owned();
        let pending = PendingImport {
            stable_key: format!("task:{}:{path}", task_id.0),
            path: path.clone(),
            lifecycle: input.lifecycle,
            tags: input.tags.clone(),
            folders: input.parent_folder_id.into_iter().collect(),
            folder_path: input
                .preserve_structure
                .then(|| {
                    candidate
                        .relative_parent
                        .as_deref()
                        .and_then(|relative| import::truncate_relative_folder(relative, capacity))
                })
                .flatten()
                .map(|relative| relative.to_string_lossy().into_owned()),
            source_urls: input.source_urls.clone(),
            imported_at_ms: task.created_at_ms,
            archive: import::is_zip(&candidate.path),
        };
        if input.group_files {
            members.push(pending);
            continue;
        }
        chunk.push(TaskIngestJob {
            job: PreparedIngestJob {
                job_key: pending.stable_key.clone(),
                source_kind: "manual".into(),
                source_path: path,
                source_item_id: None,
                delete_after_ingest: input.delete_after_ingest,
                payload: PreparedIngestPayload::Pending(pending),
            },
            units: 1,
        });
        if chunk.len() >= SCAN_CHUNK && !queue(application, task_id, &mut chunk, cancelled)? {
            return Ok(());
        }
    }
    if !queue(application, task_id, &mut chunk, cancelled)? {
        return Ok(());
    }
    if !members.is_empty() {
        let units = members.len() as u64;
        let payload = if members.len() == 1 {
            PreparedIngestPayload::Pending(members.remove(0))
        } else {
            PreparedIngestPayload::PendingCollection(PendingCollectionImport {
                members,
                name: import::collection_name(&input.paths),
                modified_at_ms: task.created_at_ms,
            })
        };
        chunk.push(TaskIngestJob {
            job: PreparedIngestJob {
                job_key: format!("task:{}:collection", task_id.0),
                source_kind: "manual".into(),
                source_path: input.paths.join("\n"),
                source_item_id: None,
                delete_after_ingest: input.delete_after_ingest,
                payload,
            },
            units,
        });
        if !queue(application, task_id, &mut chunk, cancelled)? {
            return Ok(());
        }
    }
    if input.watch_source_folder {
        import::attach_source_folder_watch(application, &input, &mut folders)?;
    }
    library
        .complete_task_scan(task_id, now_ms())
        .map_err(|error| error.to_string())
}

/// Queue a chunk of scanned files. While the task is paused the scan waits
/// here; returns false when the scan should end (stopped, or closing).
fn queue(
    application: &LibraryApplication,
    task: TaskId,
    chunk: &mut Vec<TaskIngestJob>,
    cancelled: &dyn Fn() -> bool,
) -> Result<bool, String> {
    loop {
        if cancelled() {
            return Ok(false);
        }
        match application
            .library()
            .task_state(task)
            .map_err(|error| error.to_string())?
        {
            Some(TaskState::Running) => break,
            Some(TaskState::Paused) => std::thread::sleep(PAUSED_SCAN_POLL),
            _ => return Ok(false),
        }
    }
    if !chunk.is_empty() {
        application
            .library()
            .enqueue_task_jobs(task, chunk, &chrono::Utc::now().to_rfc3339())
            .map_err(|error| error.to_string())?;
        chunk.clear();
    }
    Ok(true)
}

/// Read, hash and probe queued task files ahead of the ingest batches, a few
/// at a time. A file that cannot be read fails its job with the reason.
pub async fn prepare_task_jobs(application: &LibraryApplication) -> Result<usize, String> {
    let library = application.library();
    let backlog = library
        .prepared_task_backlog()
        .map_err(|error| error.to_string())?;
    if backlog >= PREPARE_AHEAD {
        return Ok(0);
    }
    let jobs = library
        .unprepared_task_jobs(PREPARE_AHEAD - backlog)
        .map_err(|error| error.to_string())?;
    let folders = Mutex::new(FolderCache::new());
    let mut prepared = 0;
    for chunk in jobs.chunks(PREPARE_CONCURRENCY) {
        // Pausing or stopping takes effect between these small groups.
        let mut running = HashSet::new();
        for task in chunk.iter().filter_map(|job| job.task_id).collect::<HashSet<_>>() {
            if library.task_state(task).map_err(|error| error.to_string())? == Some(TaskState::Running) {
                running.insert(task);
            }
        }
        let results = futures_util::future::join_all(
            chunk
                .iter()
                .filter(|job| job.task_id.is_some_and(|task| running.contains(&task)))
                .map(|job| async {
                    (
                        job.ingest_job_id,
                        prepare_pending(application, &job.payload, &folders).await,
                    )
                }),
        )
        .await;
        if results.is_empty() {
            break;
        }
        prepared += results.len();
        library
            .settle_unprepared_task_jobs(&results, &chrono::Utc::now().to_rfc3339())
            .map_err(|error| error.to_string())?;
    }
    Ok(prepared)
}

/// The importable form of a pending payload; other payloads are returned as
/// they are.
pub async fn prepare_pending(
    application: &LibraryApplication,
    payload: &PreparedIngestPayload,
    folders: &Mutex<FolderCache>,
) -> Result<PreparedIngestPayload, String> {
    match payload {
        PreparedIngestPayload::Pending(pending) => {
            let members = prepare_member(application, pending, folders).await?;
            let name = pending.archive.then(|| file_stem(&pending.path)).flatten();
            Ok(payload_for(members, name, pending.imported_at_ms))
        }
        PreparedIngestPayload::PendingCollection(collection) => {
            let mut members = Vec::with_capacity(collection.members.len());
            for member in &collection.members {
                members.extend(prepare_member(application, member, folders).await?);
            }
            Ok(payload_for(
                members,
                collection.name.clone(),
                collection.modified_at_ms,
            ))
        }
        other => Ok(other.clone()),
    }
}

async fn prepare_member(
    application: &LibraryApplication,
    pending: &PendingImport,
    folders: &Mutex<FolderCache>,
) -> Result<Vec<PreparedImport>, String> {
    let fields = ImportFields {
        lifecycle: pending.lifecycle,
        tags: &pending.tags,
        source_urls: &pending.source_urls,
    };
    let path = Path::new(&pending.path);
    let mut members = if pending.archive {
        import::prepare_archive(application, path, &fields, None, pending.imported_at_ms).await?
    } else {
        vec![
            import::prepare_import(
                path,
                &fields,
                None,
                pending.stable_key.clone(),
                pending.imported_at_ms,
            )
            .await?,
        ]
    };
    // Kept source folders are created only for files that could be read.
    let parent = pending.folders.first().copied();
    let folder = match &pending.folder_path {
        Some(relative) => {
            let mut cache = folders.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            import::ensure_relative_folder(application, parent, Some(Path::new(relative)), &mut cache)?
        }
        None => parent,
    };
    for member in &mut members {
        member.folders = folder.into_iter().collect();
    }
    Ok(members)
}

fn payload_for(
    mut members: Vec<PreparedImport>,
    name: Option<String>,
    modified_at_ms: i64,
) -> PreparedIngestPayload {
    if members.len() == 1 {
        PreparedIngestPayload::Item(members.remove(0))
    } else {
        PreparedIngestPayload::Collection(PreparedCollectionImport {
            members,
            cover_index: 0,
            existing_root_id: None,
            name,
            modified_at_ms,
        })
    }
}

fn file_stem(path: &str) -> Option<String> {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
}

/// Undo the tasks marked for it. Runs on the ingest worker, between batches,
/// so none of a task's jobs is being imported meanwhile.
pub fn settle_reverts(application: &LibraryApplication) -> Result<(), String> {
    let library = application.library();
    for task in library.tasks_to_revert().map_err(|error| error.to_string())? {
        library
            .revert_task(task, now_ms())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Scan every open task whose scan has not finished, one after another.
pub fn scan_pending_tasks(
    application: &LibraryApplication,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let tasks = application
        .library()
        .tasks_needing_scan()
        .map_err(|error| error.to_string())?;
    for task in tasks {
        if cancelled() {
            return Ok(());
        }
        if let Err(error) = scan_import_task(application, &task, cancelled) {
            tracing::warn!(%error, task_id = task.task_id.0, "Import scan failed");
            application
                .library()
                .fail_task(task.task_id, &error, now_ms())
                .map_err(|failure| failure.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use image::{Rgb, RgbImage};
    use picto_library::{Lifecycle, RootId};

    use super::*;
    use crate::library_import::{analyze_folder_tree, FolderTreeAnalysisInput};

    fn png(path: &Path, color: [u8; 3]) {
        RgbImage::from_pixel(4, 4, Rgb(color)).save(path).unwrap();
    }

    fn input(paths: Vec<String>, group_files: bool) -> ManualImportInput {
        ManualImportInput {
            paths,
            tags: vec!["source:manual".into()],
            source_urls: vec!["https://example.test/manual".into()],
            lifecycle: Lifecycle::Inbox,
            parent_folder_id: None,
            preserve_structure: false,
            include_subfolders: true,
            include_folders_without_media: false,
            watch_source_folder: false,
            delete_after_ingest: false,
            group_files,
            background: false,
        }
    }

    fn folder_input(source: &Path) -> ManualImportInput {
        let mut input = input(vec![source.to_string_lossy().into_owned()], false);
        input.preserve_structure = true;
        input
    }

    /// Run the import workers until nothing is left to do; the roots made.
    async fn drive(application: &LibraryApplication) -> Vec<RootId> {
        let mut roots = Vec::new();
        for _ in 0..50 {
            scan_pending_tasks(application, &|| false).unwrap();
            settle_reverts(application).unwrap();
            let prepared = prepare_task_jobs(application).await.unwrap();
            let report = crate::library_ingest_runtime::run_batch(application, 64).unwrap();
            application.library().settle_finished_tasks(now_ms()).unwrap();
            roots.extend(report.root_ids);
            if prepared == 0 && report.claimed == 0 {
                break;
            }
        }
        roots
    }

    fn task(application: &LibraryApplication, started: &ImportStarted) -> TaskRecord {
        application
            .library()
            .task(TaskId(started.task_id))
            .unwrap()
            .unwrap()
    }

    fn root_count(application: &LibraryApplication) -> u64 {
        let snapshot = application.library().projections().snapshot();
        [picto_library::RootKind::Media, picto_library::RootKind::Collection]
            .iter()
            .filter_map(|kind| snapshot.root_kinds.get(kind))
            .map(|roots| roots.len())
            .sum()
    }

    #[tokio::test]
    async fn starting_an_import_records_a_task_without_reading_any_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Downloads");
        fs::create_dir(&source).unwrap();
        for index in 0..3u8 {
            png(&source.join(format!("{index}.png")), [index, 20, 30]);
        }
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        let started = start_manual_import(&application, &folder_input(&source)).unwrap();
        let created = task(&application, &started);
        assert_eq!(created.state, TaskState::Running);
        assert_eq!(created.title, "Downloads");
        assert_eq!(created.total_units, 0);

        // The scan queues every file without reading one.
        scan_pending_tasks(&application, &|| false).unwrap();
        let scanned = task(&application, &started);
        assert!(scanned.scan_complete);
        assert_eq!((scanned.total_units, scanned.remaining_units), (3, 3));
        assert_eq!(crate::library_ingest_runtime::run_batch(&application, 64).unwrap().claimed, 0);

        drive(&application).await;
        let finished = task(&application, &started);
        assert_eq!(finished.state, TaskState::Finished);
        assert_eq!((finished.added_units, finished.done_units()), (3, 3));
    }

    #[tokio::test]
    async fn manual_import_keeps_the_requested_status_tags_and_sources() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.png");
        png(&source, [10, 20, 30]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        start_manual_import(&application, &input(vec![source.to_string_lossy().into_owned()], false))
            .unwrap();
        let roots = drive(&application).await;
        assert_eq!(roots.len(), 1);
        let details = application.library().details(roots[0]).unwrap();
        assert_eq!(details.lifecycle, Lifecycle::Inbox);
        assert_eq!(details.root.source_urls, vec!["https://example.test/manual"]);
        assert_eq!(details.tag_ids.len(), 1);
    }

    #[tokio::test]
    async fn manual_import_expands_zip_to_collection_and_cleans_staging() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.png");
        let second = directory.path().join("second.png");
        png(&first, [10, 20, 30]);
        png(&second, [30, 20, 10]);
        let archive = directory.path().join("images.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("nested/first.png", options).unwrap();
        std::io::copy(&mut fs::File::open(&first).unwrap(), &mut zip).unwrap();
        zip.start_file("second.png", options).unwrap();
        std::io::copy(&mut fs::File::open(&second).unwrap(), &mut zip).unwrap();
        zip.finish().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        start_manual_import(&application, &input(vec![archive.to_string_lossy().into_owned()], false))
            .unwrap();
        let roots = drive(&application).await;
        assert_eq!(roots.len(), 1);
        let details = application.library().details(roots[0]).unwrap();
        assert_eq!(details.root.kind, picto_library::RootKind::Collection);
        assert_eq!(details.media.len(), 2);
        assert!(archive.exists());
        let staging = application.root().join("temp/archive-import");
        assert!(fs::read_dir(staging).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn folder_import_does_not_expand_zip_files() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Downloads");
        fs::create_dir(&source).unwrap();
        let mut zip = zip::ZipWriter::new(fs::File::create(source.join("images.zip")).unwrap());
        zip.start_file("image.png", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"not needed because the archive must not be opened").unwrap();
        zip.finish().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        let started = start_manual_import(&application, &folder_input(&source)).unwrap();
        drive(&application).await;
        let stopped = task(&application, &started);
        assert_eq!(stopped.state, TaskState::Stopped);
        assert!(stopped.error.unwrap().contains("No supported media files"));
        assert!(!application.root().join("temp/archive-import").exists());
    }

    #[tokio::test]
    async fn grouped_manual_import_is_one_collection_counting_every_file() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.png");
        let second = directory.path().join("second.png");
        png(&first, [10, 20, 30]);
        png(&second, [30, 20, 10]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        let started = start_manual_import(
            &application,
            &input(
                vec![first.to_string_lossy().into_owned(), second.to_string_lossy().into_owned()],
                true,
            ),
        )
        .unwrap();
        let roots = drive(&application).await;
        assert_eq!(roots.len(), 1);
        let details = application.library().details(roots[0]).unwrap();
        assert_eq!(details.root.kind, picto_library::RootKind::Collection);
        assert_eq!(details.media.len(), 2);
        assert_eq!(details.tag_ids.len(), 1);
        let finished = task(&application, &started);
        assert_eq!((finished.total_units, finished.added_units), (2, 2));
    }

    #[tokio::test]
    async fn folder_import_imports_valid_media_and_fails_an_unreadable_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Downloads");
        fs::create_dir(&source).unwrap();
        png(&source.join("a-good.png"), [10, 20, 30]);
        fs::write(source.join("z-empty.png"), []).unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        let started = start_manual_import(&application, &folder_input(&source)).unwrap();
        let roots = drive(&application).await;
        assert_eq!(roots.len(), 1);
        let finished = task(&application, &started);
        assert_eq!(finished.state, TaskState::Finished);
        assert_eq!((finished.added_units, finished.failed_units), (1, 1));
        let failures = application.library().task_failures(TaskId(started.task_id), 10).unwrap();
        assert_eq!(failures.len(), 1);
        assert!(failures[0].0.ends_with("z-empty.png"));
        let folder = application
            .navigation()
            .unwrap()
            .folders
            .into_iter()
            .find(|folder| folder.name == "Downloads")
            .expect("the successful file should create its preserved folder");
        let imported = application.library().details(roots[0]).unwrap();
        assert_eq!(imported.folder_ids, vec![folder.folder_id]);
    }

    #[tokio::test]
    async fn an_import_of_only_unreadable_files_creates_no_folders() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Downloads");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("empty.png"), []).unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        let started = start_manual_import(&application, &folder_input(&source)).unwrap();
        drive(&application).await;
        let finished = task(&application, &started);
        assert_eq!((finished.added_units, finished.failed_units), (0, 1));
        assert!(application.navigation().unwrap().folders.is_empty());
    }

    #[tokio::test]
    async fn folder_import_can_watch_its_preserved_root_for_future_media() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Photos");
        fs::create_dir(&source).unwrap();
        png(&source.join("first.png"), [10, 20, 30]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let mut watched = folder_input(&source);
        watched.watch_source_folder = true;
        watched.include_subfolders = false;

        start_manual_import(&application, &watched).unwrap();
        drive(&application).await;
        let folder = application
            .navigation()
            .unwrap()
            .folders
            .into_iter()
            .find(|folder| folder.name == "Photos")
            .expect("the imported root folder should exist");
        assert!(folder.watch_enabled);
        assert_eq!(
            folder.watch_path.as_deref(),
            Some(fs::canonicalize(&source).unwrap().to_string_lossy().as_ref())
        );
        assert!(!folder.watch_subfolders);
    }

    #[tokio::test]
    async fn folder_import_consolidates_a_tree_deeper_than_eight_without_skipping_media() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Downloads");
        let mut deepest = source.clone();
        for depth in 2..=9 {
            deepest = deepest.join(format!("level-{depth}"));
        }
        fs::create_dir_all(&deepest).unwrap();
        png(&deepest.join("source.png"), [10, 20, 30]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        let analysis = analyze_folder_tree(
            &application,
            &FolderTreeAnalysisInput {
                path: source.to_string_lossy().into_owned(),
                destination_folder_id: None,
                include_subfolders: true,
                include_source_root: true,
            },
        )
        .unwrap();
        assert_eq!(analysis.source_depth, 9);
        assert_eq!(analysis.retained_depth, 8);
        assert_eq!(analysis.consolidated_levels, 1);

        start_manual_import(&application, &folder_input(&source)).unwrap();
        let roots = drive(&application).await;
        let folders = application.navigation().unwrap().folders;
        assert_eq!(folders.len(), 8);
        assert!(!folders.iter().any(|folder| folder.name == "level-9"));
        let retained = folders.iter().find(|folder| folder.name == "level-8").unwrap().folder_id;
        let details = application.library().details(roots[0]).unwrap();
        assert_eq!(details.folder_ids, vec![retained]);
    }

    #[tokio::test]
    async fn a_paused_import_waits_across_a_restart_and_continues_when_resumed() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Downloads");
        fs::create_dir(&source).unwrap();
        png(&source.join("one.png"), [10, 20, 30]);
        png(&source.join("two.png"), [30, 20, 10]);
        let root = directory.path().join("library");
        let started = {
            let application = LibraryApplication::create(&root).unwrap();
            let started = start_manual_import(&application, &folder_input(&source)).unwrap();
            scan_pending_tasks(&application, &|| false).unwrap();
            application.library().set_task_paused(TaskId(started.task_id), true).unwrap();
            assert!(drive(&application).await.is_empty());
            started
        };

        let application = LibraryApplication::open(&root).unwrap();
        assert!(drive(&application).await.is_empty());
        assert_eq!(task(&application, &started).state, TaskState::Paused);

        application.library().set_task_paused(TaskId(started.task_id), false).unwrap();
        assert_eq!(drive(&application).await.len(), 2);
        assert_eq!(task(&application, &started).state, TaskState::Finished);
    }

    #[tokio::test]
    async fn an_interrupted_scan_resumes_and_queues_each_file_once() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Downloads");
        fs::create_dir(&source).unwrap();
        png(&source.join("one.png"), [10, 20, 30]);
        png(&source.join("two.png"), [30, 20, 10]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let started = start_manual_import(&application, &folder_input(&source)).unwrap();

        // The library closes while the scan runs: nothing finishes.
        scan_pending_tasks(&application, &|| true).unwrap();
        assert!(!task(&application, &started).scan_complete);

        // A scan that ran twice (it already queued some files) adds no duplicates.
        let record = task(&application, &started);
        scan_import_task(&application, &record, &|| false).unwrap();
        scan_import_task(&application, &record, &|| false).unwrap();
        assert_eq!(task(&application, &started).total_units, 2);
        assert_eq!(drive(&application).await.len(), 2);
    }

    #[tokio::test]
    async fn stopping_keeps_what_was_imported_and_drops_the_rest() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Downloads");
        fs::create_dir(&source).unwrap();
        png(&source.join("one.png"), [10, 20, 30]);
        png(&source.join("two.png"), [30, 20, 10]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let started = start_manual_import(&application, &folder_input(&source)).unwrap();
        scan_pending_tasks(&application, &|| false).unwrap();
        // Import one file, then stop.
        let library = application.library();
        let first = library.unprepared_task_jobs(1).unwrap();
        let payload = prepare_pending(&application, &first[0].payload, &Mutex::default()).await.unwrap();
        library
            .settle_unprepared_task_jobs(&[(first[0].ingest_job_id, Ok(payload))], "2026-10-12T00:00:00Z")
            .unwrap();
        assert_eq!(crate::library_ingest_runtime::run_batch(&application, 64).unwrap().ingested, 1);

        library.stop_task(TaskId(started.task_id), false, now_ms()).unwrap();
        assert!(drive(&application).await.is_empty());
        let stopped = task(&application, &started);
        assert_eq!(stopped.state, TaskState::Stopped);
        assert_eq!((stopped.added_units, stopped.remaining_units), (1, 0));
        assert_eq!(root_count(&application), 1);
    }

    #[tokio::test]
    async fn undo_removes_only_what_the_import_created_and_the_tags_it_added() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Downloads");
        fs::create_dir(&source).unwrap();
        png(&source.join("existing.png"), [10, 20, 30]);
        png(&source.join("new.png"), [30, 20, 10]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        // existing.png is already in the library, without the import's tag.
        let mut first = input(vec![source.join("existing.png").to_string_lossy().into_owned()], false);
        first.tags = Vec::new();
        start_manual_import(&application, &first).unwrap();
        let existing = drive(&application).await;
        assert_eq!(existing.len(), 1);

        let started = start_manual_import(&application, &folder_input(&source)).unwrap();
        drive(&application).await;
        let finished = task(&application, &started);
        assert_eq!((finished.added_units, finished.existing_units), (1, 1));
        assert_eq!(root_count(&application), 2);
        assert_eq!(application.library().details(existing[0]).unwrap().tag_ids.len(), 1);

        application.library().stop_task(TaskId(started.task_id), true, now_ms()).unwrap();
        drive(&application).await;
        assert_eq!(task(&application, &started).state, TaskState::Reverted);
        assert_eq!(root_count(&application), 1);
        let kept = application.library().details(existing[0]).unwrap();
        assert!(kept.tag_ids.is_empty());
    }

    #[tokio::test]
    async fn an_import_that_moves_its_files_cannot_be_undone() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("one.png");
        png(&source, [10, 20, 30]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let mut moving = input(vec![source.to_string_lossy().into_owned()], false);
        moving.delete_after_ingest = true;

        let started = start_manual_import(&application, &moving).unwrap();
        assert!(!task(&application, &started).revertible);
        assert!(application
            .library()
            .stop_task(TaskId(started.task_id), true, now_ms())
            .is_err());
    }

    #[tokio::test]
    async fn a_background_import_does_not_appear_as_a_task() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("one.png");
        png(&source, [10, 20, 30]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let mut seeded = input(vec![source.to_string_lossy().into_owned()], false);
        seeded.background = true;

        start_manual_import(&application, &seeded).unwrap();
        assert_eq!(drive(&application).await.len(), 1);
        assert!(application.library().tasks(false).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_second_import_waits_until_the_first_one_ends() {
        let directory = tempfile::tempdir().unwrap();
        let first_source = directory.path().join("one.png");
        let second_source = directory.path().join("two.png");
        png(&first_source, [10, 20, 30]);
        png(&second_source, [40, 50, 60]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let first = input(vec![first_source.to_string_lossy().into_owned()], false);
        let second = input(vec![second_source.to_string_lossy().into_owned()], false);

        let started = start_manual_import(&application, &first).unwrap();
        assert_eq!(
            crate::tasks::foreground_tasks(&application).unwrap()[0].blocks,
            vec![crate::tasks::IMPORT_ACTION.to_owned()]
        );
        assert!(start_manual_import(&application, &second).is_err());
        // A paused import still holds its place.
        application
            .library()
            .set_task_paused(TaskId(started.task_id), true)
            .unwrap();
        assert!(start_manual_import(&application, &second).is_err());
        // Seeding never waits.
        let mut seeded = second.clone();
        seeded.background = true;
        start_manual_import(&application, &seeded).unwrap();

        application
            .library()
            .stop_task(TaskId(started.task_id), false, now_ms())
            .unwrap();
        assert!(crate::tasks::foreground_tasks(&application).unwrap()[0].blocks.is_empty());
        start_manual_import(&application, &second).unwrap();
    }
}
