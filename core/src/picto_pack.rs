//! Portable Picto Pack archives.
//!
//! Packs contain original media bytes and user-owned library metadata only.
//! Subscription/authentication state and source provenance never cross the
//! library boundary.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use picto_library::selection::SelectionTarget;
use picto_library::{
    FolderId, FolderRecord, ImmutableMediaFacts, Lifecycle, PreparedCollectionImport,
    PreparedImport, Rating, RootDetails, RootId, RootKind, SmartFolderId,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::write::SimpleFileOptions;

use crate::blob_store::mime_to_extension;
use crate::library_application::LibraryApplication;

const FORMAT: &str = "picto-pack";
const VERSION: u32 = 1;
const MANIFEST_PATH: &str = "manifest.json";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PictoPackSource {
    Items { target: SelectionTarget },
    Folder { folder_id: FolderId },
    SmartFolder { smart_folder_id: SmartFolderId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PictoPackExportRequest {
    pub source: PictoPackSource,
    pub output_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PictoPackImportRequest {
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PictoPackSummary {
    pub name: String,
    pub source_kind: String,
    pub root_count: usize,
    pub media_count: usize,
    pub folder_count: usize,
    pub smart_folder_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PictoPackExportResult {
    pub output_path: PathBuf,
    pub summary: PictoPackSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PictoPackImportResult {
    pub imported_roots: usize,
    pub imported_media: usize,
    pub imported_folders: usize,
    pub imported_smart_folders: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    pack_id: String,
    created_at_ms: i64,
    name: String,
    source_kind: String,
    roots: Vec<PackRoot>,
    folders: Vec<PackFolder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackRoot {
    stable_key: String,
    kind: RootKind,
    name: String,
    notes: Option<String>,
    source_urls: Vec<String>,
    imported_at_ms: i64,
    captured_at_ms: Option<i64>,
    modified_at_ms: i64,
    lifecycle: Lifecycle,
    rating: Rating,
    tags: Vec<String>,
    folder_keys: Vec<String>,
    cover_index: usize,
    media: Vec<PackMedia>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackMedia {
    stable_key: String,
    name: String,
    notes: Option<String>,
    facts: ImmutableMediaFacts,
    blob_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackFolder {
    stable_key: String,
    parent_key: Option<String>,
    name: String,
    icon: Option<String>,
    color: Option<String>,
    notes: Option<String>,
    display_order: i64,
    auto_tags: Vec<String>,
    cover_root_key: Option<String>,
}

struct ResolvedSource {
    root_ids: Vec<RootId>,
    selected_folder_ids: HashSet<FolderId>,
    name: String,
    source_kind: String,
}

pub fn inspect(request: &PictoPackImportRequest) -> Result<PictoPackSummary, String> {
    let manifest = read_manifest(&request.path)?;
    Ok(summary(&manifest))
}

pub fn export(
    application: &LibraryApplication,
    request: &PictoPackExportRequest,
) -> Result<PictoPackExportResult, String> {
    reject_library_path(application.root(), &request.output_path)?;
    let manifest = build_manifest(application, &request.source)?;
    if manifest.roots.is_empty() {
        return Err("A Picto Pack must contain at least one library item".into());
    }
    let result_summary = summary(&manifest);
    let parent = request
        .output_path
        .parent()
        .ok_or_else(|| "Picto Pack output path must have a parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create export folder: {error}"))?;
    let temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("Could not stage Picto Pack: {error}"))?;
    let file = temporary
        .reopen()
        .map_err(|error| format!("Could not open staged Picto Pack: {error}"))?;
    let mut archive = zip::ZipWriter::new(file);
    let manifest_options =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let media_options =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive
        .start_file(MANIFEST_PATH, manifest_options)
        .map_err(|error| format!("Could not write Picto Pack manifest: {error}"))?;
    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("Could not encode Picto Pack manifest: {error}"))?;
    archive
        .write_all(&manifest_json)
        .map_err(|error| format!("Could not write Picto Pack manifest: {error}"))?;

    let mut written = HashSet::new();
    for media in manifest.roots.iter().flat_map(|root| &root.media) {
        if !written.insert(media.blob_path.clone()) {
            continue;
        }
        let path = PathBuf::from(&media_path(application, &media.facts)?);
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("Could not read original {}: {error}", path.display()))?;
        if metadata.len() != media.facts.size_bytes {
            return Err(format!(
                "Original size changed while exporting {}",
                media.facts.content_hash
            ));
        }
        archive
            .start_file(&media.blob_path, media_options)
            .map_err(|error| format!("Could not add media to Picto Pack: {error}"))?;
        let mut input = File::open(&path)
            .map_err(|error| format!("Could not open original {}: {error}", path.display()))?;
        std::io::copy(&mut input, &mut archive)
            .map_err(|error| format!("Could not copy original into Picto Pack: {error}"))?;
    }
    let output = archive
        .finish()
        .map_err(|error| format!("Could not finish Picto Pack: {error}"))?;
    output
        .sync_all()
        .map_err(|error| format!("Could not sync Picto Pack: {error}"))?;
    drop(output);
    if request.output_path.exists() {
        fs::remove_file(&request.output_path)
            .map_err(|error| format!("Could not replace existing Picto Pack: {error}"))?;
    }
    temporary
        .persist(&request.output_path)
        .map_err(|error| format!("Could not publish Picto Pack: {}", error.error))?;
    Ok(PictoPackExportResult {
        output_path: request.output_path.clone(),
        summary: result_summary,
    })
}

const IMPORT_STACK_BYTES: usize = 16 * 1024 * 1024;

pub fn import(
    application: &LibraryApplication,
    request: &PictoPackImportRequest,
) -> Result<PictoPackImportResult, String> {
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("picto-pack-import".into())
            .stack_size(IMPORT_STACK_BYTES)
            .spawn_scoped(scope, || import_on_current_thread(application, request))
            .map_err(|error| format!("Could not start Picto Pack import: {error}"))?;
        worker
            .join()
            .map_err(|_| "Picto Pack import terminated unexpectedly".to_string())?
    })
}

fn import_on_current_thread(
    application: &LibraryApplication,
    request: &PictoPackImportRequest,
) -> Result<PictoPackImportResult, String> {
    let manifest = read_manifest(&request.path)?;
    let staging =
        tempfile::tempdir().map_err(|error| format!("Could not stage Picto Pack: {error}"))?;
    let mut archive = zip::ZipArchive::new(
        File::open(&request.path).map_err(|error| format!("Could not open Picto Pack: {error}"))?,
    )
    .map_err(|error| format!("Could not read Picto Pack: {error}"))?;

    // Validate every declared original before the first library mutation. A
    // corrupt archive cannot leave empty folders or partially imported roots.
    let mut staged_media = HashMap::<String, PathBuf>::new();
    for media in manifest.roots.iter().flat_map(|root| &root.media) {
        if staged_media.contains_key(&media.blob_path) {
            continue;
        }
        let staged = extract_media(&mut archive, staging.path(), media)?;
        staged_media.insert(media.blob_path.clone(), staged);
    }

    let portable_folders = manifest.folders.clone();
    let folder_ids = import_folders(application, &portable_folders)?;
    let mut imported_by_key = HashMap::<String, RootId>::new();
    let mut imported_media = 0;
    for root in &manifest.roots {
        let folders = root
            .folder_keys
            .iter()
            .filter_map(|key| folder_ids.get(key).copied())
            .collect::<Vec<_>>();
        let mut members = Vec::with_capacity(root.media.len());
        for media in &root.media {
            let staged = staged_media
                .get(&media.blob_path)
                .ok_or_else(|| format!("Picto Pack is missing {}", media.blob_path))?;
            application
                .blobs()
                .write_original_from_path(
                    &media.facts.content_hash,
                    staged,
                    Some(mime_to_extension(&media.facts.mime)),
                )
                .map_err(|error| format!("Could not store Picto Pack original: {error}"))?;
            let file_path = application
                .blobs()
                .original_path_with_ext(
                    &media.facts.content_hash,
                    Some(mime_to_extension(&media.facts.mime)),
                )
                .map_err(|error| format!("Could not resolve imported original: {error}"))?;
            members.push(PreparedImport {
                stable_key: media.stable_key.clone(),
                media_name: media.name.clone(),
                file_path: file_path.to_string_lossy().into_owned(),
                facts: media.facts.clone(),
                lifecycle: root.lifecycle,
                rating: root.rating,
                notes: media.notes.clone(),
                tags: root.tags.clone(),
                folders: folders.clone(),
                source_urls: root.source_urls.clone(),
                source_identity: None,
                imported_at_ms: root.imported_at_ms,
                captured_at_ms: root.captured_at_ms,
            });
            imported_media += 1;
        }
        let (root_id, _) = match root.kind {
            RootKind::Media => application.library().ingest(
                members
                    .first()
                    .ok_or_else(|| "Media root has no media".to_string())?,
            ),
            RootKind::Collection => {
                application
                    .library()
                    .ingest_collection(&PreparedCollectionImport {
                        members,
                        cover_index: root.cover_index,
                        existing_root_id: None,
                        name: Some(root.name.clone()),
                        modified_at_ms: root.modified_at_ms,
                    })
            }
        }
        .map_err(|error| format!("Could not ingest Picto Pack item: {error}"))?;
        restore_root_metadata(application, root_id, root)?;
        imported_by_key.insert(root.stable_key.clone(), root_id);
    }
    restore_folder_metadata(
        application,
        &portable_folders,
        &folder_ids,
        &imported_by_key,
    )?;
    Ok(PictoPackImportResult {
        imported_roots: imported_by_key.len(),
        imported_media,
        imported_folders: folder_ids.len(),
        imported_smart_folders: 0,
    })
}

fn build_manifest(
    application: &LibraryApplication,
    source: &PictoPackSource,
) -> Result<Manifest, String> {
    let all_folders = application
        .library()
        .folders()
        .map_err(|error| error.to_string())?;
    let all_smart = application
        .library()
        .smart_folders()
        .map_err(|error| error.to_string())?;
    let resolved = resolve_source(application, source, &all_folders, &all_smart)?;
    let tag_names = application
        .library()
        .tags()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|tag| {
            (
                tag.tag_id,
                crate::tag_name::format(&tag.namespace, &tag.subname),
            )
        })
        .collect::<HashMap<_, _>>();
    let folder_keys = all_folders
        .iter()
        .map(|folder| (folder.folder_id, folder.stable_key.clone()))
        .collect::<HashMap<_, _>>();
    let mut roots = Vec::with_capacity(resolved.root_ids.len());
    for root_id in resolved.root_ids {
        let details = application
            .library()
            .details(root_id)
            .map_err(|error| error.to_string())?;
        roots.push(pack_root(&details, &tag_names, &folder_keys)?);
    }
    if matches!(source, PictoPackSource::SmartFolder { .. }) {
        for root in &mut roots {
            root.folder_keys.clear();
        }
    }
    let root_keys = roots
        .iter()
        .map(|root| root.stable_key.as_str())
        .collect::<HashSet<_>>();
    let included_folders =
        collect_included_folders(&all_folders, &resolved.selected_folder_ids, &roots);
    let mut folders = Vec::new();
    for folder in all_folders
        .iter()
        .filter(|folder| included_folders.contains(&folder.folder_id))
    {
        let auto_tags = application
            .library()
            .folder_auto_tags(folder.folder_id)
            .map_err(|error| error.to_string())?;
        folders.push(PackFolder {
            stable_key: folder.stable_key.clone(),
            parent_key: folder.parent_id.and_then(|id| {
                included_folders
                    .contains(&id)
                    .then(|| folder_keys[&id].clone())
            }),
            name: folder.name.clone(),
            icon: folder.icon.clone(),
            color: folder.color.clone(),
            notes: folder.notes.clone(),
            display_order: folder.display_order,
            auto_tags,
            cover_root_key: folder
                .cover_root_id
                .and_then(|id| application.library().details(id).ok())
                .map(|details| details.root.stable_key)
                .filter(|key| root_keys.contains(key.as_str())),
        });
    }
    Ok(Manifest {
        format: FORMAT.into(),
        version: VERSION,
        pack_id: Uuid::new_v4().to_string(),
        created_at_ms: chrono::Utc::now().timestamp_millis(),
        name: resolved.name,
        source_kind: resolved.source_kind,
        roots,
        folders,
    })
}

fn resolve_source(
    application: &LibraryApplication,
    source: &PictoPackSource,
    folders: &[FolderRecord],
    smart: &[picto_library::SmartFolderRecord],
) -> Result<ResolvedSource, String> {
    match source {
        PictoPackSource::Items { target } => {
            let target = target.clone();
            let roots = application
                .library()
                .auxiliary_read_consistent(
                    picto_library::database::WorkPriority::VisibleRead,
                    move |connection, projection| {
                        picto_library::selection::resolve_ordered(connection, projection, &target)
                    },
                )
                .map_err(|error| error.to_string())?;
            Ok(ResolvedSource {
                root_ids: roots,
                selected_folder_ids: HashSet::new(),
                name: "Picto Pack".into(),
                source_kind: "items".into(),
            })
        }
        PictoPackSource::Folder { folder_id } => {
            let selected = folders
                .iter()
                .find(|folder| folder.folder_id == *folder_id)
                .ok_or_else(|| format!("Folder {} does not exist", folder_id.0))?;
            let mut ids = HashSet::from([*folder_id]);
            loop {
                let before = ids.len();
                let children = folders
                    .iter()
                    .filter_map(|folder| {
                        folder
                            .parent_id
                            .filter(|parent| ids.contains(parent))
                            .map(|_| folder.folder_id)
                    })
                    .collect::<Vec<_>>();
                ids.extend(children);
                if ids.len() == before {
                    break;
                }
            }
            let ordered = folders
                .iter()
                .filter(|folder| ids.contains(&folder.folder_id))
                .map(|folder| folder.folder_id)
                .collect::<Vec<_>>();
            let roots = application
                .library()
                .auxiliary_read_consistent(
                    picto_library::database::WorkPriority::VisibleRead,
                    move |_connection, projection| {
                        let mut seen = HashSet::new();
                        Ok(ordered
                            .iter()
                            .flat_map(|id| {
                                projection
                                    .folder_orders
                                    .get(id)
                                    .into_iter()
                                    .flat_map(|roots| roots.iter().copied())
                            })
                            .filter(|root| seen.insert(*root))
                            .collect::<Vec<_>>())
                    },
                )
                .map_err(|error| error.to_string())?;
            Ok(ResolvedSource {
                root_ids: roots,
                selected_folder_ids: ids,
                name: selected.name.clone(),
                source_kind: "folder".into(),
            })
        }
        PictoPackSource::SmartFolder { smart_folder_id } => {
            let selected = smart
                .iter()
                .find(|folder| folder.smart_folder_id == *smart_folder_id)
                .ok_or_else(|| format!("Smart folder {} does not exist", smart_folder_id.0))?;
            let id = *smart_folder_id;
            let roots = application
                .library()
                .auxiliary_read_consistent(
                    picto_library::database::WorkPriority::VisibleRead,
                    move |_connection, projection| {
                        Ok(projection
                            .smart_results
                            .get(&id.0)
                            .map(|values| values.iter().map(RootId).collect())
                            .unwrap_or_default())
                    },
                )
                .map_err(|error| error.to_string())?;
            Ok(ResolvedSource {
                root_ids: roots,
                selected_folder_ids: HashSet::new(),
                name: selected.name.clone(),
                source_kind: "smart_folder".into(),
            })
        }
    }
}

fn pack_root(
    details: &RootDetails,
    tags: &HashMap<picto_library::TagId, String>,
    folders: &HashMap<FolderId, String>,
) -> Result<PackRoot, String> {
    let cover_index = details
        .media
        .iter()
        .position(|media| media.media_id == details.root.cover_media_id)
        .unwrap_or(0);
    let media = details
        .media
        .iter()
        .map(|media| {
            let extension = mime_to_extension(&media.facts.mime);
            PackMedia {
                stable_key: if details.root.kind == RootKind::Media {
                    details.root.stable_key.clone()
                } else {
                    format!("{}:media:{}", details.root.stable_key, media.media_id.0)
                },
                name: media.media_name.clone(),
                notes: media.media_notes.clone(),
                facts: media.facts.clone(),
                blob_path: format!("blobs/{}.{}", media.facts.content_hash, extension),
            }
        })
        .collect();
    Ok(PackRoot {
        stable_key: details.root.stable_key.clone(),
        kind: details.root.kind,
        name: details.root.name.clone(),
        notes: details.root.notes.clone(),
        source_urls: details.root.source_urls.clone(),
        imported_at_ms: details.root.imported_at_ms,
        captured_at_ms: details.root.captured_at_ms,
        modified_at_ms: details.root.modified_at_ms,
        lifecycle: details.lifecycle,
        rating: details.rating,
        tags: details
            .tag_ids
            .iter()
            .filter_map(|id| tags.get(id).cloned())
            .collect(),
        folder_keys: details
            .folder_ids
            .iter()
            .filter_map(|id| folders.get(id).cloned())
            .collect(),
        cover_index,
        media,
    })
}

fn collect_included_folders(
    folders: &[FolderRecord],
    selected: &HashSet<FolderId>,
    roots: &[PackRoot],
) -> HashSet<FolderId> {
    let by_key = folders
        .iter()
        .map(|folder| (folder.stable_key.as_str(), folder.folder_id))
        .collect::<HashMap<_, _>>();
    let mut ids = selected.clone();
    for key in roots.iter().flat_map(|root| &root.folder_keys) {
        if let Some(id) = by_key.get(key.as_str()) {
            ids.insert(*id);
        }
    }
    loop {
        let before = ids.len();
        let parents = folders
            .iter()
            .filter(|folder| ids.contains(&folder.folder_id))
            .filter_map(|folder| folder.parent_id)
            .collect::<Vec<_>>();
        ids.extend(parents);
        if ids.len() == before {
            return ids;
        }
    }
}

fn import_folders(
    application: &LibraryApplication,
    folders: &[PackFolder],
) -> Result<HashMap<String, FolderId>, String> {
    let mut pending = folders.to_vec();
    pending.sort_by_key(|folder| folder.display_order);
    let mut ids = HashMap::new();
    while !pending.is_empty() {
        let before = pending.len();
        let mut next = Vec::new();
        for folder in pending {
            let parent_id = match folder.parent_key.as_ref() {
                None => None,
                Some(key) => match ids.get(key).copied() {
                    Some(id) => Some(id),
                    None => {
                        next.push(folder);
                        continue;
                    }
                },
            };
            let (id, _) = application
                .library()
                .create_folder(&folder.name, parent_id)
                .map_err(|error| error.to_string())?;
            application
                .library()
                .set_folder_metadata(
                    id,
                    folder.icon.as_deref(),
                    folder.color.as_deref(),
                    folder.notes.as_deref(),
                )
                .map_err(|error| error.to_string())?;
            ids.insert(folder.stable_key, id);
        }
        if next.len() == before {
            return Err("Picto Pack folder hierarchy contains a cycle".into());
        }
        pending = next;
    }
    Ok(ids)
}

fn restore_root_metadata(
    application: &LibraryApplication,
    root_id: RootId,
    root: &PackRoot,
) -> Result<(), String> {
    let target = SelectionTarget::Explicit {
        root_ids: vec![root_id],
    };
    application
        .library()
        .rename_root(root_id, &root.name, root.modified_at_ms)
        .map_err(|error| error.to_string())?;
    application
        .library()
        .set_notes(&target, root.notes.clone(), root.modified_at_ms)
        .map_err(|error| error.to_string())?;
    application
        .library()
        .set_source_urls(&target, root.source_urls.clone(), root.modified_at_ms)
        .map_err(|error| error.to_string())?;
    application
        .library()
        .set_rating(&target, root.rating)
        .map_err(|error| error.to_string())?;
    application
        .library()
        .set_lifecycle(&target, root.lifecycle)
        .map_err(|error| error.to_string())?;
    for tag in &root.tags {
        application
            .library()
            .add_tag(&target, tag)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn restore_folder_metadata(
    application: &LibraryApplication,
    folders: &[PackFolder],
    ids: &HashMap<String, FolderId>,
    roots: &HashMap<String, RootId>,
) -> Result<(), String> {
    let auto_tag_names = folders
        .iter()
        .flat_map(|folder| folder.auto_tags.iter().cloned())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if !auto_tag_names.is_empty() {
        application
            .library()
            .ensure_tag_definitions(&auto_tag_names)
            .map_err(|error| error.to_string())?;
    }
    let tags_by_name = application
        .library()
        .tags()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|tag| {
            (
                crate::tag_name::format(&tag.namespace, &tag.subname),
                tag.tag_id,
            )
        })
        .collect::<HashMap<_, _>>();
    for folder in folders {
        let id = ids[&folder.stable_key];
        let auto_tags = folder
            .auto_tags
            .iter()
            .map(|name| {
                tags_by_name
                    .get(name)
                    .copied()
                    .ok_or_else(|| format!("Folder auto-tag `{name}` could not be restored"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !auto_tags.is_empty() {
            application
                .library()
                .set_folder_auto_tags(id, auto_tags, chrono::Utc::now().timestamp_millis())
                .map_err(|error| error.to_string())?;
        }
        if let Some(root_id) = folder
            .cover_root_key
            .as_ref()
            .and_then(|key| roots.get(key))
            .copied()
        {
            application
                .library()
                .set_folder_cover(id, root_id)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn extract_media<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    staging: &Path,
    media: &PackMedia,
) -> Result<PathBuf, String> {
    let mut entry = archive
        .by_name(&media.blob_path)
        .map_err(|_| format!("Picto Pack is missing {}", media.blob_path))?;
    if entry.is_dir()
        || entry.compression() != zip::CompressionMethod::Stored
        || entry.size() != media.facts.size_bytes
    {
        return Err(format!(
            "Picto Pack media storage does not match its manifest: {}",
            media.blob_path
        ));
    }
    let path = staging.join(format!("{}.part", media.facts.content_hash));
    let mut output = File::create(&path)
        .map_err(|error| format!("Could not stage Picto Pack media: {error}"))?;
    let mut hasher = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = entry
            .read(&mut buffer)
            .map_err(|error| format!("Could not read Picto Pack media: {error}"))?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(read as u64)
            .ok_or_else(|| "Picto Pack media size overflow".to_string())?;
        if copied > media.facts.size_bytes {
            return Err(format!(
                "Picto Pack media exceeds declared size: {}",
                media.blob_path
            ));
        }
        hasher.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("Could not stage Picto Pack media: {error}"))?;
    }
    if copied != media.facts.size_bytes
        || !media
            .facts
            .content_hash
            .eq_ignore_ascii_case(&hex::encode(hasher.finalize()))
    {
        return Err(format!(
            "Picto Pack media checksum failed: {}",
            media.blob_path
        ));
    }
    output
        .sync_all()
        .map_err(|error| format!("Could not sync staged Picto Pack media: {error}"))?;
    Ok(path)
}

fn read_manifest(path: &Path) -> Result<Manifest, String> {
    let file = File::open(path).map_err(|error| format!("Could not open Picto Pack: {error}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("Could not read Picto Pack: {error}"))?;
    let entry = archive
        .by_name(MANIFEST_PATH)
        .map_err(|_| "Picto Pack has no manifest".to_string())?;
    if entry.size() > MAX_MANIFEST_BYTES {
        return Err("Picto Pack manifest is too large".into());
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read Picto Pack manifest: {error}"))?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Picto Pack manifest is invalid: {error}"))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &Manifest) -> Result<(), String> {
    if manifest.format != FORMAT || manifest.version != VERSION {
        return Err(format!(
            "Unsupported Picto Pack format/version: {}/{}",
            manifest.format, manifest.version
        ));
    }
    let mut paths = HashMap::<&str, (&str, u64)>::new();
    for media in manifest.roots.iter().flat_map(|root| &root.media) {
        if !media.blob_path.starts_with("blobs/")
            || media.blob_path.contains("..")
            || media.blob_path.contains('\\')
        {
            return Err("Picto Pack contains an unsafe media path".into());
        }
        if let Some((hash, size)) = paths.insert(
            &media.blob_path,
            (&media.facts.content_hash, media.facts.size_bytes),
        ) {
            if hash != media.facts.content_hash || size != media.facts.size_bytes {
                return Err("Picto Pack reuses one blob path for different media".into());
            }
        }
    }
    Ok(())
}

fn summary(manifest: &Manifest) -> PictoPackSummary {
    let mut seen = HashSet::new();
    let total_bytes = manifest
        .roots
        .iter()
        .flat_map(|root| &root.media)
        .filter(|media| seen.insert(media.blob_path.as_str()))
        .map(|media| media.facts.size_bytes)
        .sum();
    PictoPackSummary {
        name: manifest.name.clone(),
        source_kind: manifest.source_kind.clone(),
        root_count: manifest.roots.len(),
        media_count: manifest.roots.iter().map(|root| root.media.len()).sum(),
        folder_count: manifest.folders.len(),
        smart_folder_count: 0,
        total_bytes,
    }
}

fn media_path(
    application: &LibraryApplication,
    facts: &ImmutableMediaFacts,
) -> Result<String, String> {
    application
        .blobs()
        .original_path_with_ext(&facts.content_hash, Some(mime_to_extension(&facts.mime)))
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| format!("Could not resolve original: {error}"))
}

fn reject_library_path(library_root: &Path, output: &Path) -> Result<(), String> {
    let library_root = fs::canonicalize(library_root)
        .map_err(|error| format!("Could not resolve library path: {error}"))?;
    let parent = output
        .parent()
        .ok_or_else(|| "Picto Pack output path has no parent".to_string())?;
    let parent = fs::canonicalize(parent)
        .map_err(|error| format!("Could not resolve Picto Pack output folder: {error}"))?;
    if parent.starts_with(library_root) {
        return Err("Cannot export a Picto Pack into the library directory".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(
        application: &LibraryApplication,
        stable_key: &str,
        name: &str,
        bytes: &[u8],
        folder: FolderId,
    ) -> PreparedImport {
        let hash = hex::encode(Sha256::digest(bytes));
        application
            .blobs()
            .write_original(&hash, bytes, Some("png"))
            .unwrap();
        let path = application
            .blobs()
            .original_path_with_ext(&hash, Some("png"))
            .unwrap();
        PreparedImport {
            stable_key: stable_key.into(),
            media_name: name.into(),
            file_path: path.to_string_lossy().into_owned(),
            facts: ImmutableMediaFacts {
                mime: "image/png".into(),
                size_bytes: bytes.len() as u64,
                width: Some(1),
                height: Some(1),
                duration_ms: None,
                frame_count: None,
                content_hash: hash,
                perceptual_hash: None,
                palette: Vec::new(),
            },
            lifecycle: Lifecycle::Inbox,
            rating: Rating::Four,
            notes: Some(format!("notes for {name}")),
            tags: vec!["creator:tester".into(), "portable".into()],
            folders: vec![folder],
            source_urls: vec![format!("https://example.test/{stable_key}")],
            source_identity: Some(picto_library::SourceIdentity {
                source_key: "test-provider".into(),
                source_item_key: stable_key.into(),
                source_text: Some("must not travel".into()),
                source_attempt_id: None,
            }),
            imported_at_ms: 1_700_000_000_000,
            captured_at_ms: Some(1_690_000_000_000),
        }
    }

    #[test]
    fn folder_pack_round_trips_originals_and_portable_metadata_without_provenance() {
        let temporary = tempfile::tempdir().unwrap();
        let source = LibraryApplication::create(temporary.path().join("source.library")).unwrap();
        let destination =
            LibraryApplication::create(temporary.path().join("destination.library")).unwrap();
        let (folder_id, _) = source.library().create_folder("Portfolio", None).unwrap();
        source
            .library()
            .set_folder_metadata(
                folder_id,
                Some("folder"),
                Some("#123456"),
                Some("folder notes"),
            )
            .unwrap();
        let (auto_tags, _) = source
            .library()
            .ensure_tag_definitions(&["series:portable-series".into()])
            .unwrap();
        source
            .library()
            .set_folder_auto_tags(folder_id, auto_tags, 1_700_000_000_000)
            .unwrap();

        let item = prepared(&source, "portable-item", "One", b"one-original", folder_id);
        source.library().ingest(&item).unwrap();
        let first = prepared(
            &source,
            "portable-member-one",
            "Two A",
            b"two-a-original",
            folder_id,
        );
        let second = prepared(
            &source,
            "portable-member-two",
            "Two B",
            b"two-b-original",
            folder_id,
        );
        source
            .library()
            .ingest_collection(&PreparedCollectionImport {
                members: vec![first, second],
                cover_index: 1,
                existing_root_id: None,
                name: Some("Two-part collection".into()),
                modified_at_ms: 1_710_000_000_000,
            })
            .unwrap();
        let source_provenance_count = source
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    connection
                        .query_row("SELECT COUNT(*) FROM source_provenance", [], |row| {
                            row.get::<_, i64>(0)
                        })
                        .map_err(Into::into)
                },
            )
            .unwrap();
        assert_eq!(source_provenance_count, 3);

        let pack = temporary.path().join("portfolio.picto-pack");
        let exported = export(
            &source,
            &PictoPackExportRequest {
                source: PictoPackSource::Folder { folder_id },
                output_path: pack.clone(),
            },
        )
        .unwrap();
        assert_eq!(exported.summary.root_count, 2);
        assert_eq!(exported.summary.media_count, 3);
        assert_eq!(
            inspect(&PictoPackImportRequest { path: pack.clone() }).unwrap(),
            exported.summary
        );

        let imported = import(&destination, &PictoPackImportRequest { path: pack }).unwrap();
        assert_eq!(imported.imported_roots, 2);
        assert_eq!(imported.imported_media, 3);
        assert_eq!(imported.imported_folders, 1);
        let folders = destination.library().folders().unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Portfolio");
        assert_eq!(folders[0].notes.as_deref(), Some("folder notes"));
        assert_eq!(folders[0].watch_path, None);
        assert_eq!(
            destination
                .library()
                .folder_auto_tags(folders[0].folder_id)
                .unwrap(),
            vec!["series:portable-series"]
        );
        let roots = destination
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    let mut statement =
                        connection.prepare("SELECT root_id FROM library_root ORDER BY name")?;
                    let values = statement
                        .query_map([], |row| row.get::<_, u32>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    Ok(values)
                },
            )
            .unwrap();
        assert_eq!(roots.len(), 2);
        let details = roots
            .into_iter()
            .map(|id| destination.library().details(RootId(id)).unwrap())
            .collect::<Vec<_>>();
        assert!(details
            .iter()
            .all(|value| value.lifecycle == Lifecycle::Inbox));
        assert!(details.iter().all(|value| value.rating == Rating::Four));
        assert_eq!(
            details.iter().map(|value| value.media.len()).sum::<usize>(),
            3
        );
        let provenance_count = destination
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    connection
                        .query_row("SELECT COUNT(*) FROM source_provenance", [], |row| {
                            row.get::<_, i64>(0)
                        })
                        .map_err(Into::into)
                },
            )
            .unwrap();
        assert_eq!(provenance_count, 0);

        let source_roots = source
            .library()
            .auxiliary_read(
                picto_library::database::WorkPriority::VisibleRead,
                |connection| {
                    let mut statement = connection.prepare("SELECT root_id FROM library_root")?;
                    let values = statement
                        .query_map([], |row| row.get::<_, u32>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    Ok(values.into_iter().map(RootId).collect::<Vec<_>>())
                },
            )
            .unwrap();
        source
            .library()
            .set_lifecycle(
                &SelectionTarget::Explicit {
                    root_ids: source_roots,
                },
                Lifecycle::Active,
            )
            .unwrap();
        let (smart_id, _) = source
            .library()
            .create_smart_folder(picto_library::SmartFolderInput {
                name: "Everything portable".into(),
                parent_id: None,
                icon: Some("sparkles".into()),
                color: Some("#abcdef".into()),
                notes: Some("this rule does not travel".into()),
                view: picto_library::predicate::ViewQuerySpec::default(),
            })
            .unwrap();
        let smart_pack = temporary.path().join("smart.picto-pack");
        let smart_export = export(
            &source,
            &PictoPackExportRequest {
                source: PictoPackSource::SmartFolder {
                    smart_folder_id: smart_id,
                },
                output_path: smart_pack.clone(),
            },
        )
        .unwrap();
        assert_eq!(smart_export.summary.root_count, 2);
        assert_eq!(smart_export.summary.folder_count, 0);
        assert_eq!(smart_export.summary.smart_folder_count, 0);
        let smart_destination =
            LibraryApplication::create(temporary.path().join("smart-destination.library")).unwrap();
        let smart_import = import(
            &smart_destination,
            &PictoPackImportRequest { path: smart_pack },
        )
        .unwrap();
        assert_eq!(smart_import.imported_roots, 2);
        assert_eq!(smart_import.imported_folders, 0);
        assert!(smart_destination.library().folders().unwrap().is_empty());
        assert!(smart_destination
            .library()
            .smart_folders()
            .unwrap()
            .is_empty());
    }
}
