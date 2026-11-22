//! Merges another library into the open library.
//!
//! The open library is the one that is kept. The other library is only read:
//! its database is opened read-only, and nothing in its folder is created,
//! changed or removed.
//!
//! A merge is a foreground task of the import kind, so it uses the import
//! queue, its progress, Pause, and the undo of a stopped task
//! (`import_tasks.rs`, `picto-library/src/mutation/tasks.rs`):
//!
//! 1. [`start`] checks the other library, makes a backup of the open
//!    library's database and records the task.
//! 2. The scan worker calls [`scan`]. It creates the other library's tags,
//!    folders and smart folders in the open library, where the open library
//!    does not have them yet, and queues one import job for each item.
//! 3. The ingest worker copies each file and its thumbnail into the open
//!    library's blob store and imports the item
//!    (`library_ingest_runtime.rs`).
//! 4. [`settle`] undoes a merge that was stopped or that could not import a
//!    file, and removes the backup when the merge has finished or has been
//!    undone.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use picto_library::content::{self, ContentSmartFolder, Outline};
use picto_library::{
    FolderId, MergeTaskInput, PreparedCollectionImport, PreparedImport, PreparedIngestJob,
    PreparedIngestPayload, RootDetails, RootKind, SmartFolderId, SmartFolderInput, TagId, TaskId,
    TaskIngestJob, TaskRecord, TaskState,
};
use serde::{Deserialize, Serialize};

use crate::blob_store::{library_original_path, mime_to_extension};
use crate::library_application::LibraryApplication;

/// The label in the file name of the backup that is made before a merge:
/// `library.sqlite.pre-merge-<milliseconds since 1970>.backup`.
const BACKUP_LABEL: &str = "pre-merge";
/// How many items the scan queues in one transaction.
const SCAN_CHUNK: usize = 256;

/// The arguments of `library.merge.inspect` and `library.merge.start`: the
/// folder of the other library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeRequest {
    pub path: PathBuf,
}

/// What a library holds, as the merge dialog shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeSummary {
    pub name: String,
    /// Files that stand alone, and collections.
    pub items: u64,
    /// Files, counting each member of a collection.
    pub files: u64,
}

/// The answer of `library.merge.start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeStarted {
    pub task_id: i64,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Returns the name of the library in the folder `root`: the folder's name
/// without `.library`.
fn library_name(root: &Path) -> String {
    root.file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Library".into())
}

/// A library that is open for reading only. While this value exists, the
/// library's lock file is held, so that no Picto opens the library.
struct Source {
    root: PathBuf,
    connection: rusqlite::Connection,
    _lock: Option<fs::File>,
}

/// Opens the library in the folder `root` for reading.
///
/// - The library's lock file (`state::LIBRARY_LOCK_FILE`) is locked when it
///   exists, and the open fails when another application holds it. The file
///   is not created when it is missing, because nothing may be added to the
///   library's folder.
/// - The database is opened with SQLite's `immutable` flag, which takes no
///   lock and makes no journal or shared-memory file. That flag reads the
///   database file alone. So a library whose write-ahead log still holds
///   changes, which is the case after Picto was ended without closing the
///   library, is refused.
/// - The schema must be the current one (`picto_library::schema::validate`),
///   as for a library that is opened. An older schema is not updated here.
fn open_source(root: &Path) -> Result<Source, String> {
    let root = fs::canonicalize(root)
        .map_err(|error| format!("Could not find the library {}: {error}", root.display()))?;
    let database = root.join("library.sqlite");
    if !database.is_file() {
        return Err(format!("{} is not a Picto library.", root.display()));
    }
    let lock = match fs::File::open(root.join(crate::state::LIBRARY_LOCK_FILE)) {
        Ok(file) => match file.try_lock() {
            Ok(()) => Some(file),
            Err(fs::TryLockError::WouldBlock) => {
                return Err(format!(
                    "{} ({}). Close it there, then merge again.",
                    crate::state::LIBRARY_IN_USE_MESSAGE,
                    root.display()
                ))
            }
            Err(fs::TryLockError::Error(error)) => {
                return Err(format!(
                    "Could not lock the library ({}): {error}",
                    root.display()
                ))
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "Could not lock the library ({}): {error}",
                root.display()
            ))
        }
    };
    let log = root.join("library.sqlite-wal");
    if fs::metadata(&log).is_ok_and(|log| log.len() > 0) {
        return Err(format!(
            "The library {} was not closed completely. Open it in Picto, close it, and merge again.",
            root.display()
        ));
    }
    let mut address = url::Url::from_file_path(&database)
        .map_err(|()| format!("{} is not an absolute path", database.display()))?;
    address.set_query(Some("mode=ro&immutable=1"));
    let connection = rusqlite::Connection::open_with_flags(
        address.as_str(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_URI
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("{}: {error}", database.display()))?;
    picto_library::schema::validate(&connection).map_err(|error| error.to_string())?;
    Ok(Source {
        root,
        connection,
        _lock: lock,
    })
}

/// Reads how many items the library in the folder `request.path` holds. The
/// library is opened for reading as [`open_source`] describes, so a library
/// that another Picto has open, or whose schema is not the current one, is
/// refused with the reason.
pub fn inspect(request: &MergeRequest) -> Result<MergeSummary, String> {
    let source = open_source(&request.path)?;
    let (items, files) =
        content::counts(&source.connection).map_err(|error| error.to_string())?;
    Ok(MergeSummary {
        name: library_name(&source.root),
        items,
        files,
    })
}

/// Starts the merge of the library in the folder `request.path` into the
/// open library. It refuses the open library itself, a library that cannot
/// be read ([`open_source`]), and a start while an import is running. Then
/// it writes the backup of the open library's database and records the
/// task. The backup is written before the task is recorded, so that it holds
/// the library exactly as it was before the merge.
pub fn start(
    application: &LibraryApplication,
    request: &MergeRequest,
) -> Result<MergeStarted, String> {
    let kept = fs::canonicalize(application.root())
        .map_err(|error| format!("Failed to resolve library path: {error}"))?;
    let other = fs::canonicalize(&request.path).map_err(|error| {
        format!(
            "Could not find the library {}: {error}",
            request.path.display()
        )
    })?;
    if other == kept {
        return Err("A library cannot be merged with itself.".into());
    }
    if other.starts_with(&kept) || kept.starts_with(&other) {
        return Err("A library cannot be merged with a library inside it.".into());
    }
    let _start = crate::tasks::lock_import_start(application)?;
    crate::tasks::ensure_can_start(application, crate::tasks::IMPORT_ACTION)?;
    drop(open_source(&other)?);
    let library = application.library();
    let backup = library
        .backup_database(BACKUP_LABEL)
        .map_err(|error| format!("Could not back up the library before the merge: {error}"))?;
    let input = MergeTaskInput {
        library: other.clone(),
        backup: backup.clone(),
    };
    match library.create_task(
        crate::import_tasks::IMPORT_TASK_KIND,
        &library_name(&other),
        &input.to_json(),
        true,
        false,
        now_ms(),
    ) {
        Ok(task) => Ok(MergeStarted { task_id: task.0 }),
        Err(error) => {
            let _ = fs::remove_file(&backup);
            Err(error.to_string())
        }
    }
}

/// Does the scan of a merge task. It creates the tags, folders and smart
/// folders of the other library that the open library lacks, and queues one
/// import job for each item of the other library. After an interruption it
/// runs again from the start: what it already created is found again, and a
/// job that is already queued is kept.
///
/// It returns early, leaving the scan to run again later, when `cancelled`
/// says that the library is closing, and it returns for good once the task
/// is stopped.
pub(crate) fn scan(
    application: &LibraryApplication,
    task: &TaskRecord,
    input: &MergeTaskInput,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let library = application.library();
    let task_id = task.task_id;
    let source = open_source(&input.library)?;
    let outline = content::outline(&source.connection).map_err(|error| error.to_string())?;

    let names = outline
        .tags
        .iter()
        .map(|tag| tag.name.clone())
        .collect::<Vec<_>>();
    if !names.is_empty() {
        library
            .ensure_tag_definitions_for_task(&names, task_id)
            .map_err(|error| error.to_string())?;
    }
    let tags = {
        let snapshot = library.projections().snapshot();
        outline
            .tags
            .iter()
            .filter_map(|tag| Some((tag.tag_id, *snapshot.tag_ids_by_name.get(&tag.name)?)))
            .collect::<HashMap<TagId, TagId>>()
    };

    let mut folders = HashMap::<FolderId, FolderId>::new();
    for folder in &outline.folders {
        if !crate::import_tasks::scan_checkpoint(application, task_id, cancelled)? {
            return Ok(());
        }
        let parent = match folder.parent_id {
            Some(parent) => Some(*folders.get(&parent).ok_or_else(|| {
                format!("The folder “{}” of the other library has no parent.", folder.name)
            })?),
            None => None,
        };
        let auto_tags = folder
            .auto_tag_ids
            .iter()
            .filter_map(|tag_id| tags.get(tag_id).copied())
            .collect::<Vec<_>>();
        let (folder_id, _) = library
            .create_folder_like_for_task(folder, parent, &auto_tags, task_id)
            .map_err(|error| error.to_string())?;
        folders.insert(folder.folder_id, folder_id);
    }

    add_smart_folders(application, task_id, &input.library, &outline, &tags, &folders)?;

    let tag_names = outline
        .tags
        .iter()
        .map(|tag| (tag.tag_id, tag.name.as_str()))
        .collect::<HashMap<_, _>>();
    let mut chunk = Vec::with_capacity(SCAN_CHUNK);
    // The reader's callback returns the library's error type, so what goes
    // wrong in it is kept here and returned after the reading has stopped.
    let mut failure = None;
    let mut running = true;
    content::for_each_root(&source.connection, |details| {
        match job(task_id, &source.root, &details, &tag_names, &folders) {
            Ok(job) => chunk.push(job),
            Err(error) => {
                failure = Some(error);
                return Ok(false);
            }
        }
        if chunk.len() >= SCAN_CHUNK {
            match crate::import_tasks::queue(application, task_id, &mut chunk, cancelled) {
                Ok(true) => {}
                Ok(false) => {
                    running = false;
                    return Ok(false);
                }
                Err(error) => {
                    failure = Some(error);
                    return Ok(false);
                }
            }
        }
        Ok(true)
    })
    .map_err(|error| error.to_string())?;
    if let Some(error) = failure {
        return Err(error);
    }
    if !running || !crate::import_tasks::queue(application, task_id, &mut chunk, cancelled)? {
        return Ok(());
    }
    library
        .complete_task_scan(task_id, now_ms())
        .map_err(|error| error.to_string())
}

/// Creates the smart folders of the other library in the open library. A
/// smart folder is left out when its parent already has one with the same
/// name and the same rules. When the parent has one with the same name and
/// other rules, the new one gets the other library's name in brackets after
/// its own. The rules name tags and folders by their ids, so they are
/// rewritten to the open library's ids (`picto_library::translate_view`).
fn add_smart_folders(
    application: &LibraryApplication,
    task_id: TaskId,
    other: &Path,
    outline: &Outline,
    tags: &HashMap<TagId, TagId>,
    folders: &HashMap<FolderId, FolderId>,
) -> Result<(), String> {
    if outline.smart_folders.is_empty() {
        return Ok(());
    }
    let library = application.library();
    // The tag ids and folder ids of a rule are compared in ascending order.
    fn ordered(view: &picto_library::predicate::ViewQuerySpec) -> Option<picto_library::predicate::ViewQuerySpec> {
        picto_library::translate_view(view, &|tag| Some(tag), &|folder| Some(folder))
    }
    let mut present = library
        .smart_folders()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|folder| {
            (
                folder.parent_id,
                folder.name,
                ordered(&folder.view).map(|view| view.filter),
                folder.smart_folder_id,
            )
        })
        .collect::<Vec<_>>();
    let mut placed = HashMap::<SmartFolderId, SmartFolderId>::new();
    for smart in &outline.smart_folders {
        let ContentSmartFolder {
            smart_folder_id,
            parent_id,
            name,
            icon,
            color,
            notes,
            view,
            ..
        } = smart;
        let parent = match parent_id {
            Some(parent) => match placed.get(parent) {
                Some(parent) => Some(*parent),
                // The parent was left out, so its children are left out too.
                None => continue,
            },
            None => None,
        };
        let Some(view) = picto_library::translate_view(
            view,
            &|tag| tags.get(&tag).copied(),
            &|folder| folders.get(&folder).copied(),
        ) else {
            tracing::warn!(name, "A smart folder names a tag or folder that the other library does not have; it is not merged");
            continue;
        };
        let same = present.iter().find(|(held_parent, held_name, held_rules, _)| {
            *held_parent == parent && held_name == name && held_rules.as_ref() == Some(&view.filter)
        });
        if let Some((_, _, _, held)) = same {
            placed.insert(*smart_folder_id, *held);
            continue;
        }
        let taken = |candidate: &str| {
            present
                .iter()
                .any(|(held_parent, held_name, _, _)| *held_parent == parent && held_name == candidate)
        };
        let mut new_name = name.clone();
        let mut number = 1;
        while taken(&new_name) {
            new_name = if number == 1 {
                format!("{name} ({})", library_name(other))
            } else {
                format!("{name} ({} {number})", library_name(other))
            };
            number += 1;
        }
        let (created, _) = library
            .create_smart_folder_for_task(
                SmartFolderInput {
                    name: new_name.clone(),
                    parent_id: parent,
                    icon: icon.clone(),
                    color: color.clone(),
                    notes: notes.clone(),
                    view: view.clone(),
                },
                task_id,
            )
            .map_err(|error| error.to_string())?;
        present.push((parent, new_name, Some(view.filter), created));
        placed.insert(*smart_folder_id, created);
    }
    Ok(())
}

/// Returns the import job for one item of the other library.
///
/// The item's key in the open library starts with `merge:<task>:`. The keys
/// of two libraries are not comparable, so an item of the other library is
/// recognised as one that the open library holds only by the hash of its
/// file. `file_path` is the file in the other library's blob store, from
/// which the ingest worker copies it. Each file of a collection carries the
/// collection's tags, folders, rating, status, dates and source links, as
/// the import of a collection expects.
fn job(
    task_id: TaskId,
    other: &Path,
    details: &RootDetails,
    tag_names: &HashMap<TagId, &str>,
    folders: &HashMap<FolderId, FolderId>,
) -> Result<TaskIngestJob, String> {
    let root = &details.root;
    let key = format!("merge:{}:{}", task_id.0, root.stable_key);
    let tags = details
        .tag_ids
        .iter()
        .filter_map(|tag_id| tag_names.get(tag_id).map(|name| (*name).to_owned()))
        .collect::<Vec<_>>();
    let folder_ids = details
        .folder_ids
        .iter()
        .filter_map(|folder_id| folders.get(folder_id).copied())
        .collect::<Vec<_>>();
    let alone = root.kind == RootKind::Media;
    let mut members = Vec::with_capacity(details.media.len());
    for media in &details.media {
        let file_path = library_original_path(
            other,
            &media.facts.content_hash,
            mime_to_extension(&media.facts.mime),
        )
        .map_err(|error| format!("The item “{}” names an invalid file: {error}", root.name))?;
        members.push(PreparedImport {
            stable_key: if alone {
                key.clone()
            } else {
                format!("{key}:media:{}", media.media_id.0)
            },
            media_name: if alone {
                root.name.clone()
            } else {
                media.media_name.clone()
            },
            file_path: file_path.to_string_lossy().into_owned(),
            facts: media.facts.clone(),
            lifecycle: details.lifecycle,
            rating: details.rating,
            notes: if alone {
                root.notes.clone().or_else(|| media.media_notes.clone())
            } else {
                media.media_notes.clone()
            },
            tags: tags.clone(),
            folders: folder_ids.clone(),
            source_urls: root.source_urls.clone(),
            source_identity: None,
            imported_at_ms: root.imported_at_ms,
            captured_at_ms: root.captured_at_ms,
        });
    }
    let source_path = members
        .first()
        .map(|member| member.file_path.clone())
        .ok_or_else(|| format!("The item “{}” has no file.", root.name))?;
    let units = members.len() as u64;
    // A collection of one file is imported as that file alone, because a
    // collection import needs two files.
    let payload = if members.len() == 1 {
        PreparedIngestPayload::Item(members.remove(0))
    } else {
        let cover_index = details
            .media
            .iter()
            .position(|media| media.media_id == root.cover_media_id)
            .unwrap_or(0);
        PreparedIngestPayload::Collection(PreparedCollectionImport {
            members,
            cover_index,
            existing_root_id: None,
            name: Some(root.name.clone()),
            modified_at_ms: root.modified_at_ms,
        })
    };
    Ok(TaskIngestJob {
        job: PreparedIngestJob {
            job_key: key,
            source_kind: "library".into(),
            source_path,
            source_item_id: None,
            delete_after_ingest: false,
            payload,
        },
        units,
    })
}

/// Keeps the promise that a merge either brings everything or changes
/// nothing, and removes the backup when it is no longer needed. The ingest
/// worker calls this before it undoes the tasks that are marked for undo.
///
/// - A merge with a file that could not be imported is marked for undo with
///   that file's error, whether it is still running or has just finished.
/// - A stopped merge is marked for undo. A merge is stopped when its scan
///   failed, for example because the other library can no longer be read.
/// - The backup of a merge that has finished or has been undone is removed.
///   While a merge runs, and when its undo did not complete, the backup
///   stays.
pub fn settle(application: &LibraryApplication) -> Result<(), String> {
    let library = application.library();
    for (task_id, state, input, failed) in
        library.merge_tasks().map_err(|error| error.to_string())?
    {
        match state {
            TaskState::Running | TaskState::Paused | TaskState::Finished if failed => {
                let reason = library
                    .task_failures(task_id, 1)
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .next()
                    .map(|(path, error)| format!("{path}: {error}"))
                    .unwrap_or_default();
                library
                    .undo_incomplete_merge(
                        task_id,
                        Some(&format!(
                            "A file of the other library could not be merged. {reason}"
                        )),
                        now_ms(),
                    )
                    .map_err(|error| error.to_string())?;
            }
            TaskState::Stopped => {
                library
                    .undo_incomplete_merge(task_id, None, now_ms())
                    .map_err(|error| error.to_string())?;
            }
            TaskState::Finished | TaskState::Reverted => {
                if input.backup.is_file() {
                    if let Err(error) = fs::remove_file(&input.backup) {
                        tracing::warn!(%error, backup = %input.backup.display(), "Could not remove the backup of a merge");
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
