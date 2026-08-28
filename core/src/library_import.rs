//! Filesystem import adapter for the canonical library ingest queue.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use picto_library::{
    FolderId, ImmutableMediaFacts, Lifecycle, PreparedCollectionImport, PreparedImport,
    PreparedIngestJob, PreparedIngestPayload, Rating,
};
use serde::{Deserialize, Serialize};

use crate::library_application::LibraryApplication;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualImportInput {
    pub paths: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source_urls: Vec<String>,
    pub lifecycle: Lifecycle,
    pub parent_folder_id: Option<FolderId>,
    #[serde(default)]
    pub preserve_structure: bool,
    #[serde(default = "default_true")]
    pub include_subfolders: bool,
    #[serde(default = "default_true")]
    pub expand_archives: bool,
    #[serde(default)]
    pub include_folders_without_media: bool,
    #[serde(default)]
    pub delete_after_ingest: bool,
    #[serde(default)]
    pub group_files: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportEnqueueReport {
    pub discovered: usize,
    pub queued: usize,
    pub already_queued: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone)]
struct ImportCandidate {
    path: PathBuf,
    relative_parent: Option<PathBuf>,
}

pub async fn enqueue_manual_import(
    application: &LibraryApplication,
    input: &ManualImportInput,
) -> Result<ImportEnqueueReport, String> {
    if input.paths.is_empty() {
        return Err("At least one import path is required".into());
    }
    let candidates = collect_manual_candidates(application.root(), input)?;
    let invocation = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now();
    let mut folders = BTreeMap::new();
    let mut report = ImportEnqueueReport {
        discovered: candidates.len(),
        ..Default::default()
    };

    if input.preserve_structure && input.include_folders_without_media {
        for value in &input.paths {
            let path = fs::canonicalize(value)
                .map_err(|error| format!("Failed to resolve import path '{value}': {error}"))?;
            if path.is_dir() {
                for relative in collect_structure_directories(&path, input.include_subfolders)? {
                    ensure_relative_folder(
                        application,
                        input.parent_folder_id,
                        Some(&relative),
                        &mut folders,
                    )?;
                }
            }
        }
    }

    let mut prepared = Vec::new();
    for (index, candidate) in candidates.into_iter().enumerate() {
        let folder_id = if input.preserve_structure {
            ensure_relative_folder(
                application,
                input.parent_folder_id,
                candidate.relative_parent.as_deref(),
                &mut folders,
            )?
        } else {
            input.parent_folder_id
        };
        if is_zip(&candidate.path) && input.expand_archives {
            match prepare_archive(&candidate.path, input, folder_id, now.timestamp_millis()).await {
                Ok(mut members) => prepared.append(&mut members),
                Err(error) if error.starts_with("Unsupported media:") => report.skipped += 1,
                Err(error) => return Err(error),
            }
            continue;
        }
        match prepare_import(
            &candidate.path,
            input,
            folder_id,
            format!("manual:{invocation}:{index}"),
            now.timestamp_millis(),
        )
        .await
        {
            Ok(value) => prepared.push(value),
            Err(error) if error.starts_with("Unsupported media:") => report.skipped += 1,
            Err(error) => return Err(error),
        }
    }

    if prepared.is_empty() {
        return Ok(report);
    }
    if input.group_files || prepared.len() > 1 && report.discovered == 1 {
        let name = collection_name(&input.paths);
        let job = PreparedIngestJob {
            job_key: format!("manual:{invocation}:collection"),
            source_kind: "manual".into(),
            source_path: input.paths.join("\n"),
            source_item_id: None,
            delete_after_ingest: input.delete_after_ingest || report.discovered == 1,
            payload: PreparedIngestPayload::Collection(PreparedCollectionImport {
                members: prepared,
                cover_index: 0,
                name,
                modified_at_ms: now.timestamp_millis(),
            }),
        };
        enqueue(application, &job, &now.to_rfc3339(), &mut report)?;
    } else {
        for (index, value) in prepared.into_iter().enumerate() {
            let job = PreparedIngestJob {
                job_key: format!("manual:{invocation}:{index}"),
                source_kind: "manual".into(),
                source_path: value.file_path.clone(),
                source_item_id: None,
                delete_after_ingest: input.delete_after_ingest,
                payload: PreparedIngestPayload::Item(value),
            };
            enqueue(application, &job, &now.to_rfc3339(), &mut report)?;
        }
    }
    Ok(report)
}

fn enqueue(
    application: &LibraryApplication,
    job: &PreparedIngestJob,
    now: &str,
    report: &mut ImportEnqueueReport,
) -> Result<(), String> {
    application
        .library()
        .enqueue_ingest_job(job, now)
        .map_err(|error| error.to_string())?;
    report.queued += 1;
    Ok(())
}

async fn prepare_archive(
    archive_path: &Path,
    input: &ManualImportInput,
    folder_id: Option<FolderId>,
    imported_at_ms: i64,
) -> Result<Vec<PreparedImport>, String> {
    let entries = crate::media_processing::archive::extract_library_files(archive_path)
        .map_err(|error| format!("Failed to extract {}: {error}", archive_path.display()))?;
    if entries.is_empty() {
        return Err(format!(
            "Unsupported media: {} contains no accepted files",
            archive_path.display()
        ));
    }
    let mut members = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let mut member = prepare_import(
            &entry.path,
            input,
            folder_id,
            format!("archive:{}:{index}", archive_path.display()),
            imported_at_ms,
        )
        .await?;
        member.media_name = Path::new(&entry.archive_name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(&member.media_name)
            .to_owned();
        members.push(member);
    }
    Ok(members)
}

async fn prepare_import(
    path: &Path,
    input: &ManualImportInput,
    folder_id: Option<FolderId>,
    stable_key: String,
    imported_at_ms: i64,
) -> Result<PreparedImport, String> {
    let prepared = crate::media_processing::PreparedMediaSource::prepare_ingest(path)
        .await
        .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
    if !prepared.caps.ingest_supported || prepared.mime_type == "application/zip" {
        return Err(format!("Unsupported media: {}", path.display()));
    }
    let hash_path = path.to_path_buf();
    let content_hash = tokio::task::spawn_blocking(move || {
        crate::media_processing::get_hash_from_path(&hash_path)
            .map(hex::encode)
            .map_err(|error| format!("Failed to hash {}: {error}", hash_path.display()))
    })
    .await
    .map_err(|error| format!("Media hash worker failed: {error}"))??;
    let size_bytes = fs::metadata(path)
        .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?
        .len();
    let captured_at_ms = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.created().or_else(|_| metadata.modified()).ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|value| i64::try_from(value.as_millis()).ok());
    Ok(PreparedImport {
        stable_key,
        media_name: path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Untitled")
            .to_owned(),
        file_path: path.to_string_lossy().into_owned(),
        facts: ImmutableMediaFacts {
            mime: prepared.mime_type,
            size_bytes,
            width: prepared.pixel_width,
            height: prepared.pixel_height,
            duration_ms: prepared.duration_ms,
            frame_count: prepared.num_frames,
            content_hash,
            perceptual_hash: None,
            palette: Vec::new(),
        },
        lifecycle: input.lifecycle,
        rating: Rating::Unrated,
        notes: None,
        tags: input.tags.clone(),
        folders: folder_id.into_iter().collect(),
        source_urls: input.source_urls.clone(),
        source_identity: None,
        imported_at_ms,
        captured_at_ms,
    })
}

fn ensure_relative_folder(
    application: &LibraryApplication,
    parent: Option<FolderId>,
    relative: Option<&Path>,
    cache: &mut BTreeMap<(Option<u32>, String), FolderId>,
) -> Result<Option<FolderId>, String> {
    let Some(relative) = relative else {
        return Ok(parent);
    };
    let mut current = parent;
    for component in relative.components() {
        let name = component.as_os_str().to_string_lossy().trim().to_owned();
        if name.is_empty() {
            continue;
        }
        let key = (current.map(|id| id.0), name.clone());
        let folder_id = if let Some(folder_id) = cache.get(&key).copied() {
            folder_id
        } else if let Some(folder_id) = application
            .navigation()?
            .folders
            .into_iter()
            .find(|folder| folder.parent_id == current && folder.name == name)
            .map(|folder| folder.folder_id)
        {
            folder_id
        } else {
            application
                .create_folder(picto_library::CreateFolderInput {
                    name: name.clone(),
                    parent_id: current,
                })?
                .folder_id
        };
        cache.insert(key, folder_id);
        current = Some(folder_id);
    }
    Ok(current)
}

fn collect_manual_candidates(
    library_root: &Path,
    input: &ManualImportInput,
) -> Result<Vec<ImportCandidate>, String> {
    let library = fs::canonicalize(library_root)
        .map_err(|error| format!("Failed to resolve library path: {error}"))?;
    let mut candidates = Vec::new();
    for value in &input.paths {
        let path = fs::canonicalize(value)
            .map_err(|error| format!("Failed to resolve import path '{value}': {error}"))?;
        if path.starts_with(&library) {
            return Err(format!("Import path must be outside the library: {}", path.display()));
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() {
            if is_media_path(&path) && !should_ignore(&path) {
                candidates.push(ImportCandidate { path, relative_parent: None });
            }
        } else if metadata.is_dir() {
            let root_name = path.file_name().map(PathBuf::from);
            candidates.extend(collect_directory(
                &path,
                input.include_subfolders,
                input.preserve_structure.then_some(root_name).flatten(),
            )?);
        }
    }
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates.dedup_by(|left, right| left.path == right.path);
    Ok(candidates)
}

fn collect_structure_directories(root: &Path, recursive: bool) -> Result<Vec<PathBuf>, String> {
    let root_name = root.file_name().map(PathBuf::from).unwrap_or_default();
    let mut directories = vec![root_name.clone()];
    if !recursive {
        return Ok(directories);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("Failed to read {}: {error}", directory.display()))?
        {
            let entry = entry.map_err(|error| format!("Failed to read directory: {error}"))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
            if !should_ignore(&path) && file_type.is_dir() && !file_type.is_symlink() {
                if let Ok(relative) = path.strip_prefix(root) {
                    directories.push(root_name.join(relative));
                }
                stack.push(path);
            }
        }
    }
    directories.sort();
    directories.dedup();
    Ok(directories)
}

fn collect_directory(
    root: &Path,
    recursive: bool,
    structure_root: Option<PathBuf>,
) -> Result<Vec<ImportCandidate>, String> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let mut entries = fs::read_dir(&directory)
            .map_err(|error| format!("Failed to read {}: {error}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Failed to read {}: {error}", directory.display()))?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if should_ignore(&path) {
                continue;
            }
            let file_type = entry
                .file_type()
                .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if recursive {
                    stack.push(path);
                }
            } else if file_type.is_file() && is_media_path(&path) {
                let relative = path
                    .strip_prefix(root)
                    .ok()
                    .and_then(Path::parent)
                    .unwrap_or_else(|| Path::new(""))
                    .to_path_buf();
                files.push(ImportCandidate {
                    path,
                    relative_parent: structure_root.as_ref().map(|base| base.join(relative)),
                });
            }
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn collection_name(paths: &[String]) -> Option<String> {
    let first = paths.first().map(Path::new)?;
    if paths.len() == 1 {
        return first.file_stem().and_then(|value| value.to_str()).map(str::to_owned);
    }
    first
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .or_else(|| Some("Imported Collection".into()))
}

fn should_ignore(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    name.starts_with('.')
        || [".part", ".partial", ".crdownload", ".tmp", ".download"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

fn is_media_path(path: &Path) -> bool {
    crate::media_processing::has_supported_extension(path)
}

fn is_zip(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};

    use super::*;

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
            expand_archives: true,
            include_folders_without_media: false,
            delete_after_ingest: false,
            group_files,
        }
    }

    #[tokio::test]
    async fn manual_import_queues_the_canonical_payload() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.png");
        png(&source, [10, 20, 30]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        let report = enqueue_manual_import(
            &application,
            &input(vec![source.to_string_lossy().into_owned()], false),
        )
        .await
        .unwrap();
        assert_eq!(report.discovered, 1);
        assert_eq!(report.queued, 1);

        let settled = crate::library_ingest_runtime::run_batch(&application, 64).unwrap();
        assert_eq!(settled.ingested, 1);
        let details = application.library().details(settled.root_ids[0]).unwrap();
        assert_eq!(details.lifecycle, Lifecycle::Inbox);
        assert_eq!(details.root.source_urls, vec!["https://example.test/manual"]);
        assert_eq!(details.tag_ids.len(), 1);
    }

    #[tokio::test]
    async fn grouped_manual_import_is_one_atomic_collection_job() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.png");
        let second = directory.path().join("second.png");
        png(&first, [10, 20, 30]);
        png(&second, [30, 20, 10]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        let report = enqueue_manual_import(
            &application,
            &input(
                vec![
                    first.to_string_lossy().into_owned(),
                    second.to_string_lossy().into_owned(),
                ],
                true,
            ),
        )
        .await
        .unwrap();
        assert_eq!(report.discovered, 2);
        assert_eq!(report.queued, 1);

        let settled = crate::library_ingest_runtime::run_batch(&application, 64).unwrap();
        assert_eq!(settled.ingested, 1);
        let details = application.library().details(settled.root_ids[0]).unwrap();
        assert_eq!(details.root.kind, picto_library::RootKind::Collection);
        assert_eq!(details.media.len(), 2);
        assert_eq!(details.tag_ids.len(), 1);
    }
}
