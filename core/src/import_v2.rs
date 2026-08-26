//! Manual and watched-folder adapters for the durable ingest queue.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{Application, Lifecycle};
use crate::folders_v2::{CreateFolderInput, FolderId};
use crate::ingest_queue_v2::{self, IngestJobSpec};
use crate::ingest_v2::{PreparedMediaInput, SourcePostInput};

const LOCAL_PROVENANCE: i64 = 1;
const WATCH_STABLE_DELAY: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
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

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
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

impl Application {
    pub async fn enqueue_manual_import(
        &self,
        input: &ManualImportInput,
    ) -> Result<ImportEnqueueReport, String> {
        if input.paths.is_empty() {
            return Err("At least one import path is required".to_string());
        }
        let candidates = collect_manual_candidates(self, input)?;
        let invocation = rand::random::<u64>();
        let grouped_post_key = format!("manual:{invocation:016x}");
        let grouped_title = manual_group_title(&candidates);
        let candidate_count = candidates.len();
        let mut folders = BTreeMap::new();
        let mut report = ImportEnqueueReport {
            discovered: candidates.len(),
            ..ImportEnqueueReport::default()
        };

        if input.preserve_structure && input.include_folders_without_media {
            for value in &input.paths {
                let path = fs::canonicalize(value)
                    .map_err(|error| format!("Failed to resolve import path '{value}': {error}"))?;
                if path.is_dir() {
                    for relative in collect_structure_directories(&path, input.include_subfolders)?
                    {
                        ensure_relative_folder(
                            self,
                            input.parent_folder_id,
                            Some(&relative),
                            &mut folders,
                        )?;
                    }
                }
            }
        }

        for (index, candidate) in candidates.into_iter().enumerate() {
            let folder_id = if input.preserve_structure {
                ensure_relative_folder(
                    self,
                    input.parent_folder_id,
                    candidate.relative_parent.as_deref(),
                    &mut folders,
                )?
            } else {
                input.parent_folder_id
            };
            let job_key = format!("manual:{invocation:016x}:{index}");
            let source = (input.group_files && candidate_count > 1).then(|| SourcePostInput {
                site_id: "manual".to_string(),
                post_key: grouped_post_key.clone(),
                item_key: format!("{grouped_post_key}:{index}"),
                position: index as i64,
                post_complete: index + 1 == candidate_count,
                force_collection: false,
                group_post: true,
                canonical_post_url: None,
                canonical_media_url: None,
                creator_name: None,
                title: grouped_title.clone(),
                description: None,
                captured_at: None,
                metadata_json: None,
            });
            match prepare_and_enqueue(
                self,
                &candidate.path,
                &job_key,
                "manual",
                input.lifecycle,
                folder_id,
                &input.tags,
                &input.source_urls,
                input.delete_after_ingest,
                input.expand_archives,
                source,
            )
            .await
            {
                Ok(true) => report.queued += 1,
                Ok(false) => report.already_queued += 1,
                Err(error) if error.starts_with("Unsupported media:") => report.skipped += 1,
                Err(error) => return Err(error),
            }
        }
        publish_queue_change(self, &report)?;
        Ok(report)
    }
}

pub async fn scan_watched_folders(
    application: &Application,
) -> Result<ImportEnqueueReport, String> {
    let auto_import_enabled = crate::settings_v2::application_settings(application)?
        .value
        .get("autoImportEnabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    if !auto_import_enabled {
        return Ok(ImportEnqueueReport::default());
    }
    let watches = application.store().read(|connection| {
        let mut statement = connection.prepare(
            "SELECT folder_id, watch_path, watch_subfolders
             FROM folder
             WHERE watch_enabled = 1 AND watch_path IS NOT NULL
             ORDER BY folder_id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    FolderId(row.get(0)?),
                    PathBuf::from(row.get::<_, String>(1)?),
                    row.get::<_, bool>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })?;
    let mut report = ImportEnqueueReport::default();
    let mut pending = Vec::new();
    let mut existing_jobs = ingest_queue_v2::existing_watch_job_keys(application)?;
    for (folder_id, root, recursive) in watches {
        for candidate in collect_directory(&root, recursive, None)? {
            report.discovered += 1;
            let metadata = match fs::metadata(&candidate.path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    report.skipped += 1;
                    continue;
                }
            };
            let job_key = watch_job_key(folder_id, &candidate.path, &metadata)?;
            if existing_jobs.contains(&job_key) {
                report.already_queued += 1;
                continue;
            }
            existing_jobs.insert(job_key.clone());
            pending.push((folder_id, candidate.path, metadata, job_key));
        }
    }
    if !pending.is_empty() {
        tokio::time::sleep(WATCH_STABLE_DELAY).await;
    }
    for (folder_id, path, metadata, job_key) in pending {
        if !file_is_stable(&path, &metadata) {
            report.skipped += 1;
            continue;
        }
        match prepare_and_enqueue(
            application,
            &path,
            &job_key,
            "watch",
            Lifecycle::Inbox,
            Some(folder_id),
            &[],
            &[],
            false,
            true,
            None,
        )
        .await
        {
            Ok(true) => report.queued += 1,
            Ok(false) => report.already_queued += 1,
            Err(error) if error.starts_with("Unsupported media:") => report.skipped += 1,
            Err(error) => return Err(error),
        }
    }
    publish_queue_change(application, &report)?;
    Ok(report)
}

fn publish_queue_change(
    application: &Application,
    report: &ImportEnqueueReport,
) -> Result<(), String> {
    if report.queued != 0 {
        application.publish(&crate::app::MutationReceipt {
            revision: application.store().revision()?,
            resources: vec![crate::app::resources::TASKS.to_string()],
            item_ids: Vec::new(),
        });
    }
    Ok(())
}

async fn prepare_and_enqueue(
    application: &Application,
    path: &Path,
    job_key: &str,
    source_kind: &str,
    lifecycle: Lifecycle,
    target_folder_id: Option<FolderId>,
    tags: &[String],
    source_urls: &[String],
    delete_after_ingest: bool,
    expand_archives: bool,
    source: Option<SourcePostInput>,
) -> Result<bool, String> {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        && expand_archives
    {
        return prepare_archive_and_enqueue(
            application,
            path,
            job_key,
            source_kind,
            lifecycle,
            target_folder_id,
            tags,
            source_urls,
        )
        .await;
    }

    let input = prepare_input(path, lifecycle, target_folder_id, tags, source_urls, source).await?;
    let result = ingest_queue_v2::enqueue(
        application,
        &IngestJobSpec {
            job_key: job_key.to_string(),
            source_kind: source_kind.to_string(),
            source_path: path.display().to_string(),
            delete_after_ingest,
            input,
        },
    )?;
    Ok(result.inserted)
}

fn manual_group_title(candidates: &[ImportCandidate]) -> Option<String> {
    let mut parents = candidates
        .iter()
        .filter_map(|candidate| candidate.path.parent());
    let parent = parents.next()?;
    if !parents.all(|candidate_parent| candidate_parent == parent) {
        return Some("Imported Collection".to_string());
    }
    parent
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .or_else(|| Some("Imported Collection".to_string()))
}

pub(crate) async fn prepare_input(
    path: &Path,
    lifecycle: Lifecycle,
    target_folder_id: Option<FolderId>,
    tags: &[String],
    source_urls: &[String],
    source: Option<SourcePostInput>,
) -> Result<PreparedMediaInput, String> {
    let prepared = crate::media_processing::PreparedMediaSource::prepare_ingest(path)
        .await
        .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
    if !prepared.caps.ingest_supported || prepared.mime_type == "application/zip" {
        return Err(format!("Unsupported media: {}", path.display()));
    }
    let captured_at = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.created().or_else(|_| metadata.modified()).ok())
        .map(|time| chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339());
    Ok(PreparedMediaInput {
        file_hash: String::new(),
        mime_type: prepared.mime_type,
        size_bytes: prepared.size_bytes.unwrap_or_default() as i64,
        pixel_width: prepared.pixel_width.map(i64::from),
        pixel_height: prepared.pixel_height.map(i64::from),
        duration_ms: prepared.duration_ms.map(|value| value as i64),
        frame_count: prepared.num_frames.map(i64::from),
        has_audio: prepared.has_audio,
        name: path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(str::to_string),
        notes: None,
        rating: None,
        source_urls: source_urls.to_vec(),
        tags: tags.to_vec(),
        provenance_mask: LOCAL_PROVENANCE,
        lifecycle,
        captured_at,
        source,
        target_folder_id: target_folder_id.map(|id| id.0),
        target_folder_ids: Vec::new(),
    })
}

async fn prepare_archive_and_enqueue(
    application: &Application,
    archive_path: &Path,
    job_key: &str,
    source_kind: &str,
    lifecycle: Lifecycle,
    target_folder_id: Option<FolderId>,
    tags: &[String],
    source_urls: &[String],
) -> Result<bool, String> {
    let post_key = format!(
        "zip:{}",
        hex::encode(
            crate::media_processing::get_hash_from_path(archive_path)
                .map_err(|error| format!("Failed to hash {}: {error}", archive_path.display()))?
        )
    );
    let title = archive_path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_string);
    let entries = crate::media_processing::archive::extract_library_files(archive_path)
        .map_err(|error| format!("Failed to extract {}: {error}", archive_path.display()))?;
    if entries.is_empty() {
        return Err(format!(
            "Unsupported media: {} contains no accepted files",
            archive_path.display()
        ));
    }

    let count = entries.len();
    let mut inserted = false;
    for (index, entry) in entries.into_iter().enumerate() {
        let source = SourcePostInput {
            site_id: "archive".to_string(),
            post_key: post_key.clone(),
            item_key: format!("{post_key}:{index}:{}", entry.archive_name),
            position: index as i64,
            post_complete: index + 1 == count,
            force_collection: true,
            group_post: true,
            canonical_post_url: None,
            canonical_media_url: None,
            creator_name: None,
            title: title.clone(),
            description: None,
            captured_at: None,
            metadata_json: None,
        };
        let mut input = prepare_input(
            &entry.path,
            lifecycle,
            target_folder_id,
            tags,
            source_urls,
            Some(source),
        )
        .await?;
        input.name = Path::new(&entry.archive_name)
            .file_stem()
            .and_then(|value| value.to_str())
            .map(str::to_string);
        let result = ingest_queue_v2::enqueue(
            application,
            &IngestJobSpec {
                job_key: format!("{job_key}:zip:{index}"),
                source_kind: source_kind.to_string(),
                source_path: entry.path.display().to_string(),
                delete_after_ingest: true,
                input,
            },
        )?;
        inserted |= result.inserted;
    }
    Ok(inserted)
}

fn collect_manual_candidates(
    application: &Application,
    input: &ManualImportInput,
) -> Result<Vec<ImportCandidate>, String> {
    let library = fs::canonicalize(application.store().library_root())
        .map_err(|error| format!("Failed to resolve library path: {error}"))?;
    let mut candidates = Vec::new();
    for value in &input.paths {
        let path = fs::canonicalize(value)
            .map_err(|error| format!("Failed to resolve import path '{value}': {error}"))?;
        if path.starts_with(&library) {
            return Err(format!(
                "Import path must be outside the library: {}",
                path.display()
            ));
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() {
            if is_media_path(&path) && !should_ignore(&path) {
                candidates.push(ImportCandidate {
                    path,
                    relative_parent: None,
                });
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
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("Failed to read {}: {error}", directory.display()))?;
        for entry in entries {
            let entry = entry
                .map_err(|error| format!("Failed to read {}: {error}", directory.display()))?;
            let path = entry.path();
            if should_ignore(&path) {
                continue;
            }
            let file_type = entry
                .file_type()
                .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
            if file_type.is_dir() && !file_type.is_symlink() {
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
                    .unwrap_or_else(|| Path::new(""));
                let relative_parent = structure_root.as_ref().map(|base| base.join(relative));
                files.push(ImportCandidate {
                    path,
                    relative_parent,
                });
            }
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn ensure_relative_folder(
    application: &Application,
    parent: Option<FolderId>,
    relative: Option<&Path>,
    cache: &mut BTreeMap<(Option<i64>, String), FolderId>,
) -> Result<Option<FolderId>, String> {
    let Some(relative) = relative else {
        return Ok(parent);
    };
    let mut current = parent;
    for component in relative.components() {
        let name = component.as_os_str().to_string_lossy().trim().to_string();
        if name.is_empty() {
            continue;
        }
        let key = (current.map(|id| id.0), name.clone());
        let folder_id = if let Some(folder_id) = cache.get(&key).copied() {
            folder_id
        } else if let Some(folder_id) = existing_folder(application, current, &name)? {
            folder_id
        } else {
            application
                .create_folder(&CreateFolderInput {
                    name: name.clone(),
                    parent_id: current,
                    folder_key: None,
                })?
                .0
        };
        cache.insert(key, folder_id);
        current = Some(folder_id);
    }
    Ok(current)
}

fn existing_folder(
    application: &Application,
    parent: Option<FolderId>,
    name: &str,
) -> Result<Option<FolderId>, String> {
    application.store().read(|connection| {
        connection
            .query_row(
                "SELECT folder_id FROM folder
                 WHERE parent_id IS ?1 AND name = ?2
                 ORDER BY folder_id LIMIT 1",
                rusqlite::params![parent.map(|id| id.0), name],
                |row| row.get::<_, i64>(0).map(FolderId),
            )
            .optional()
    })
}

fn watch_job_key(
    folder_id: FolderId,
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<String, String> {
    let modified = metadata
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(format!(
        "watch:{}:{}:{}:{}",
        folder_id.0,
        path.display(),
        metadata.len(),
        modified
    ))
}

fn file_is_stable(path: &Path, previous: &fs::Metadata) -> bool {
    fs::metadata(path)
        .map(|current| {
            current.is_file()
                && current.len() == previous.len()
                && current.modified().ok() == previous.modified().ok()
        })
        .unwrap_or(false)
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

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;

    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};

    use crate::store::Store;

    fn png(path: &Path) {
        DynamicImage::ImageRgb8(ImageBuffer::from_pixel(2, 2, Rgb([1, 2, 3])))
            .save_with_format(path, ImageFormat::Png)
            .unwrap();
    }

    fn zip_files(path: &Path, files: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        for (name, bytes) in files {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
    }

    #[tokio::test]
    async fn manual_directory_filters_files_and_preserves_folder_structure() {
        let library = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fs::create_dir(source.path().join("child")).unwrap();
        fs::create_dir(source.path().join("without-media")).unwrap();
        png(&source.path().join("one.png"));
        png(&source.path().join("child/two.png"));
        fs::write(source.path().join("child/ignored.exe"), b"ignored").unwrap();
        fs::write(source.path().join("without-media/ignored.exe"), b"ignored").unwrap();
        let application = Application::new(Arc::new(Store::open(library.path()).unwrap()));

        let report = application
            .enqueue_manual_import(&ManualImportInput {
                paths: vec![source.path().display().to_string()],
                tags: vec!["general:test".to_string()],
                source_urls: Vec::new(),
                lifecycle: Lifecycle::Inbox,
                parent_folder_id: None,
                preserve_structure: true,
                include_subfolders: true,
                expand_archives: true,
                include_folders_without_media: false,
                delete_after_ingest: false,
                group_files: false,
            })
            .await
            .unwrap();
        assert_eq!(report.discovered, 2);
        assert_eq!(report.queued, 2);
        assert_eq!(
            ingest_queue_v2::run_batch(&application, 8)
                .unwrap()
                .ingested,
            2
        );
        application
            .store()
            .read(|connection| {
                let folders = connection
                    .prepare("SELECT name FROM folder ORDER BY folder_id")?
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let memberships: i64 =
                    connection
                        .query_row("SELECT COUNT(*) FROM folder_item", [], |row| row.get(0))?;
                assert_eq!(folders.len(), 2, "folders: {folders:?}");
                assert_eq!(memberships, 2);
                Ok(())
            })
            .unwrap();
    }

    #[tokio::test]
    async fn manual_directory_can_include_folders_without_supported_media() {
        let library = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fs::create_dir(source.path().join("without-media")).unwrap();
        fs::write(source.path().join("without-media/ignored.exe"), b"ignored").unwrap();
        png(&source.path().join("one.png"));
        let application = Application::new(Arc::new(Store::open(library.path()).unwrap()));

        application
            .enqueue_manual_import(&ManualImportInput {
                paths: vec![source.path().display().to_string()],
                tags: Vec::new(),
                source_urls: Vec::new(),
                lifecycle: Lifecycle::Inbox,
                parent_folder_id: None,
                preserve_structure: true,
                include_subfolders: true,
                expand_archives: true,
                include_folders_without_media: true,
                delete_after_ingest: false,
                group_files: false,
            })
            .await
            .unwrap();

        let folder_count: i64 = application
            .store()
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM folder", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(folder_count, 2);
    }

    #[tokio::test]
    async fn manual_directory_can_skip_subfolders() {
        let library = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        fs::create_dir(source.path().join("child")).unwrap();
        png(&source.path().join("one.png"));
        png(&source.path().join("child/two.png"));
        let application = Application::new(Arc::new(Store::open(library.path()).unwrap()));

        let report = application
            .enqueue_manual_import(&ManualImportInput {
                paths: vec![source.path().display().to_string()],
                tags: Vec::new(),
                source_urls: Vec::new(),
                lifecycle: Lifecycle::Inbox,
                parent_folder_id: None,
                preserve_structure: true,
                include_subfolders: false,
                expand_archives: true,
                include_folders_without_media: false,
                delete_after_ingest: false,
                group_files: false,
            })
            .await
            .unwrap();

        assert_eq!(report.discovered, 1);
        assert_eq!(report.queued, 1);
    }

    #[tokio::test]
    async fn grouped_manual_files_become_one_visible_collection_after_completion() {
        let library = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let first_path = source.path().join("one.png");
        let second_path = source.path().join("two.png");
        png(&first_path);
        png(&second_path);
        let application = Application::new(Arc::new(Store::open(library.path()).unwrap()));

        let report = application
            .enqueue_manual_import(&ManualImportInput {
                paths: vec![
                    first_path.display().to_string(),
                    second_path.display().to_string(),
                ],
                tags: Vec::new(),
                source_urls: Vec::new(),
                lifecycle: Lifecycle::Inbox,
                parent_folder_id: None,
                preserve_structure: false,
                include_subfolders: true,
                expand_archives: true,
                include_folders_without_media: false,
                delete_after_ingest: false,
                group_files: true,
            })
            .await
            .unwrap();
        assert_eq!(report.queued, 2);

        assert_eq!(
            ingest_queue_v2::run_batch(&application, 1)
                .unwrap()
                .ingested,
            1
        );
        assert!(application.projections().inbox_bitmap().is_empty());

        assert_eq!(
            ingest_queue_v2::run_batch(&application, 1)
                .unwrap()
                .ingested,
            1
        );
        application
            .store()
            .read(|connection| {
                let roots: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM library_root lr
                     JOIN library_item li ON li.item_id = lr.item_id
                     WHERE li.kind = 'collection'",
                    [],
                    |row| row.get(0),
                )?;
                let members: i64 =
                    connection.query_row("SELECT COUNT(*) FROM collection_member", [], |row| {
                        row.get(0)
                    })?;
                assert_eq!(roots, 1);
                assert_eq!(members, 2);
                Ok(())
            })
            .unwrap();
        assert_eq!(application.projections().inbox_bitmap().len(), 1);
    }

    #[tokio::test]
    async fn clipboard_source_is_owned_after_successful_staging() {
        let library = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let staged = source.path().join("clipboard.png");
        png(&staged);
        let application = Application::new(Arc::new(Store::open(library.path()).unwrap()));

        let report = application
            .enqueue_manual_import(&ManualImportInput {
                paths: vec![staged.display().to_string()],
                tags: Vec::new(),
                source_urls: Vec::new(),
                lifecycle: Lifecycle::Inbox,
                parent_folder_id: None,
                preserve_structure: false,
                include_subfolders: true,
                expand_archives: true,
                include_folders_without_media: false,
                delete_after_ingest: true,
                group_files: false,
            })
            .await
            .unwrap();

        assert_eq!(report.queued, 1);
        assert!(!staged.exists());
        assert_eq!(
            ingest_queue_v2::run_batch(&application, 1)
                .unwrap()
                .ingested,
            1
        );
        assert!(!staged.exists());
    }

    #[tokio::test]
    async fn watched_file_is_enqueued_once_and_imports_to_inbox_folder() {
        let library = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        png(&source.path().join("watched.png"));
        let application = Application::new(Arc::new(Store::open(library.path()).unwrap()));
        let folder = application
            .create_folder(&CreateFolderInput {
                name: "Watch".to_string(),
                parent_id: None,
                folder_key: None,
            })
            .unwrap()
            .0;
        application
            .set_folder_watch(&crate::folders_v2::FolderWatchInput {
                folder_id: folder,
                path: source.path().display().to_string(),
                include_subfolders: false,
            })
            .unwrap();

        assert_eq!(scan_watched_folders(&application).await.unwrap().queued, 1);
        assert_eq!(
            scan_watched_folders(&application)
                .await
                .unwrap()
                .already_queued,
            1
        );
        let report = ingest_queue_v2::run_batch(&application, 8).unwrap();
        assert_eq!(report.ingested, 1);
        let root = report.item_ids[0].0;
        assert!(application
            .projections()
            .inbox_bitmap()
            .contains(root as u32));
        assert!(!application
            .projections()
            .folder_bitmap(folder.0)
            .contains(root as u32));
        let membership: i64 = application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT COUNT(*) FROM folder_item WHERE folder_id = ?1 AND item_id = ?2",
                    rusqlite::params![folder.0, root],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(membership, 1);
        application
            .set_lifecycle(
                &crate::app::ItemTarget::Explicit {
                    item_ids: vec![crate::app::ItemId(root)],
                },
                Lifecycle::Active,
            )
            .unwrap();
        assert!(application
            .projections()
            .folder_bitmap(folder.0)
            .contains(root as u32));
    }

    #[tokio::test]
    async fn disabled_auto_import_does_not_scan_watched_folders() {
        let library = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        png(&source.path().join("watched.png"));
        let application = Application::new(Arc::new(Store::open(library.path()).unwrap()));
        let folder = application
            .create_folder(&CreateFolderInput {
                name: "Watch".to_string(),
                parent_id: None,
                folder_key: None,
            })
            .unwrap()
            .0;
        application
            .set_folder_watch(&crate::folders_v2::FolderWatchInput {
                folder_id: folder,
                path: source.path().display().to_string(),
                include_subfolders: false,
            })
            .unwrap();
        application
            .patch_application_settings(&serde_json::json!({ "autoImportEnabled": false }))
            .unwrap();

        assert_eq!(
            scan_watched_folders(&application).await.unwrap(),
            ImportEnqueueReport::default()
        );
        assert!(ingest_queue_v2::existing_watch_job_keys(&application)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn manual_zip_expands_to_one_hidden_until_complete_collection() {
        let library = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let archive_path = source.path().join("album.zip");
        zip_files(
            &archive_path,
            &[("track.mp3", b"audio"), ("notes.txt", b"credits")],
        );
        let application = Application::new(Arc::new(Store::open(library.path()).unwrap()));

        let report = application
            .enqueue_manual_import(&ManualImportInput {
                paths: vec![archive_path.display().to_string()],
                tags: Vec::new(),
                source_urls: Vec::new(),
                lifecycle: Lifecycle::Inbox,
                parent_folder_id: None,
                preserve_structure: false,
                include_subfolders: true,
                expand_archives: true,
                include_folders_without_media: false,
                delete_after_ingest: false,
                group_files: false,
            })
            .await
            .unwrap();
        assert_eq!(report.discovered, 1);
        assert_eq!(report.queued, 1);

        assert_eq!(
            ingest_queue_v2::run_batch(&application, 1)
                .unwrap()
                .ingested,
            1
        );
        let roots_after_first: i64 = application
            .store()
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM library_root", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(roots_after_first, 0);

        assert_eq!(
            ingest_queue_v2::run_batch(&application, 8)
                .unwrap()
                .ingested,
            1
        );
        application
            .store()
            .read(|connection| {
                let (kind, label, members): (String, Option<String>, i64) = connection.query_row(
                    "SELECT li.kind, li.label,
                            (SELECT COUNT(*) FROM collection_member cm
                             WHERE cm.collection_id = li.item_id)
                     FROM library_item li
                     JOIN library_root lr ON lr.item_id = li.item_id",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?;
                assert_eq!(kind, "collection");
                assert_eq!(label.as_deref(), Some("album"));
                assert_eq!(members, 2);
                Ok(())
            })
            .unwrap();
    }
}
