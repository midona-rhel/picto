use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::blob_store::BlobStore;
use crate::background_work::DeferredWorkType;
use crate::db::LibraryDatabase;
use crate::db::projection::compiler::CompilerPlan;
use crate::db::types::{
    IngestPreparedSingle, MediaEntityPatch, PerceptualHashCandidate, TAG_PROVENANCE_MANUAL,
    TAG_PROVENANCE_UNKNOWN,
};
use crate::import::pipeline::{ImportOptions, ImportPipeline};
use crate::media_capabilities::capabilities_for_stored_media;
use crate::media_analysis;
use crate::sqlite::SqliteDatabase;
use crate::subscriptions::gallery_dl_runner::ParsedMetadata;
use crate::tags::normalize;
use crate::types::{ImportBatchResult, ImportResult};

#[derive(Debug, Clone, Copy)]
pub enum IngestSourceKind {
    Manual,
    WatchFolder,
    Subscription,
    Migration,
}

#[derive(Debug, Clone, Default)]
pub struct IngestFlags {
    pub status_changed: bool,
    pub tags_changed: bool,
    pub metadata_changed: bool,
}

impl IngestFlags {
    pub(crate) fn merge(&mut self, other: &Self) {
        self.status_changed |= other.status_changed;
        self.tags_changed |= other.tags_changed;
        self.metadata_changed |= other.metadata_changed;
    }
}

#[derive(Debug, Clone)]
pub struct SingleIngestRequest {
    pub source_kind: IngestSourceKind,
    pub path: PathBuf,
    pub tag_strings: Vec<String>,
    pub source_urls: Vec<String>,
    pub name: Option<String>,
    pub notes: Option<String>,
    pub created_at: Option<String>,
    pub initial_status: i64,
    pub skip_thumbnail: bool,
    pub tag_provenance_mask: u64,
    pub subscription_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct SingleIngestOutcome {
    pub entity_hash: String,
    pub imported_new: bool,
    pub mime: String,
    pub size: u64,
    pub has_thumbnail: bool,
    pub tags_applied: Vec<String>,
    pub flags: IngestFlags,
    pub scheduled_work: usize,
    pub entity_id: Option<i64>,
    pub file_id: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct IngestBatchSummary {
    pub imported_hashes: Vec<String>,
    pub skipped_hashes: Vec<String>,
    pub folder_ids: Vec<i64>,
    pub flags: IngestFlags,
    pub scheduled_work: usize,
}

#[derive(Debug, Clone)]
pub struct SubscriptionCollectionMember {
    pub path: PathBuf,
    pub metadata: ParsedMetadata,
    pub skip_thumbnail: bool,
}

#[derive(Debug, Clone)]
pub struct SubscriptionCollectionOutcome {
    pub collection_id: Option<i64>,
    pub collection_hash: Option<String>,
    pub imported_hashes: Vec<String>,
    pub flags: IngestFlags,
    pub scheduled_work: usize,
}

fn collect_files_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_files_recursive(&path));
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    files
}

fn collect_import_paths(root: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>), String> {
    let mut directories = Vec::<PathBuf>::new();
    let mut files = Vec::<PathBuf>::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|err| format!("Failed to read {}: {err}", directory.display()))?;
        let mut child_paths = Vec::<PathBuf>::new();
        for entry in entries {
            let entry = entry
                .map_err(|err| format!("Failed to read entry in {}: {err}", directory.display()))?;
            child_paths.push(entry.path());
        }
        child_paths.sort();

        for path in child_paths {
            if path.is_dir() {
                directories.push(path.clone());
                stack.push(path);
            } else if path.is_file() && crate::media_processing::has_supported_extension(&path) {
                files.push(path.canonicalize().unwrap_or(path));
            }
        }
    }

    directories.sort();
    files.sort();
    Ok((directories, files))
}

fn dedupe_urls(mut urls: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    urls.retain(|url| {
        let trimmed = url.trim();
        !trimmed.is_empty() && seen.insert(trimmed.to_string())
    });
    urls
}

fn merge_note_text(existing: Option<&str>, incoming: Option<&str>) -> Option<String> {
    let existing = existing.unwrap_or("").trim();
    let incoming = incoming.unwrap_or("").trim();
    match (existing.is_empty(), incoming.is_empty()) {
        (true, true) => None,
        (true, false) => Some(incoming.to_string()),
        (false, true) => Some(existing.to_string()),
        (false, false) if existing.contains(incoming) => Some(existing.to_string()),
        (false, false) => Some(format!("{existing}\n\n{incoming}")),
    }
}

fn metadata_notes_text(metadata: &ParsedMetadata) -> Option<String> {
    let mut sections = Vec::new();
    if let Some(title) = metadata.title.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        sections.push(title.to_string());
    }
    if let Some(description) = metadata
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sections.push(description.to_string());
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

fn normalize_manual_tags(tag_strings: &[String]) -> Vec<String> {
    normalize::parse_tags_ingest(tag_strings)
        .into_iter()
        .map(|(ns, st)| normalize::combine_tag(&ns, &st))
        .collect()
}

fn normalize_subscription_tags(metadata: &ParsedMetadata) -> Vec<String> {
    metadata
        .tags
        .iter()
        .map(|(ns, st)| normalize::combine_tag(ns, st))
        .collect()
}

fn build_import_options(request: &SingleIngestRequest) -> ImportOptions {
    let mut options = ImportOptions {
        initial_status: request.initial_status,
        name: request.name.clone(),
        created_at: request.created_at.clone(),
        skip_thumbnail: request.skip_thumbnail,
        ..ImportOptions::default()
    };
    options.source_urls = dedupe_urls(request.source_urls.clone());
    options.tags = request
        .tag_strings
        .iter()
        .filter_map(|raw| normalize::parse_tag(raw))
        .collect();
    if let Some(notes) = request.notes.as_deref().filter(|text| !text.trim().is_empty()) {
        let mut map = HashMap::new();
        map.insert("text".to_string(), notes.to_string());
        options.notes = Some(map);
    }
    options
}

fn prepared_from_blob_import(
    prepared: crate::import::pipeline::PreparedBlobImport,
    request: &SingleIngestRequest,
) -> IngestPreparedSingle {
    IngestPreparedSingle {
        entity_hash: prepared.hex_hash,
        name: prepared.name,
        size_bytes: prepared.size as i64,
        mime_type: prepared.mime,
        pixel_width: prepared.pixel_width,
        pixel_height: prepared.pixel_height,
        duration_ms: prepared.duration_ms,
        frame_count: prepared.num_frames,
        has_audio: prepared.has_audio,
        status: request.initial_status,
        date_created: prepared
            .created_at
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
        date_added: chrono::Utc::now().to_rfc3339(),
        has_thumbnail: prepared.has_thumbnail,
        skip_thumbnail: request.skip_thumbnail,
        notes: request
            .notes
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned),
        source_urls: dedupe_urls(request.source_urls.clone()),
        tag_strings: request.tag_strings.clone(),
        tag_provenance_mask: request.tag_provenance_mask,
        perceptual_hash: None,
    }
}

fn duplicate_review_distance_threshold() -> u32 {
    crate::state::get_state()
        .map(|state| {
            crate::settings::store::similarity_pct_to_distance(
                state.settings.get().duplicate_review_similarity_pct,
            )
        })
        .unwrap_or(crate::duplicates::phash::DEFAULT_DISTANCE_THRESHOLD)
}

fn compute_comparable_image_phash(
    path: &Path,
    mime_type: &str,
    frame_count: Option<i64>,
) -> Result<Option<String>, String> {
    let capabilities = capabilities_for_stored_media(mime_type, frame_count);
    if !capabilities.can_perceptual_hash {
        return Ok(None);
    }
    let bytes = std::fs::read(path)
        .map_err(|err| format!("Failed to read {} for perceptual hash: {err}", path.display()))?;
    crate::duplicates::phash::compute_phash_base64(&bytes)
        .map(Some)
        .map_err(|err| format!("Failed to compute perceptual hash for {}: {err}", path.display()))
}

fn work_types_for_new_ingest(
    mime_type: &str,
    frame_count: Option<i64>,
    needs_thumbnail: bool,
    has_perceptual_hash: bool,
) -> Vec<DeferredWorkType> {
    let mut work_types =
        media_analysis::derivative_work_types_for_target(mime_type, frame_count, needs_thumbnail);
    if has_perceptual_hash {
        work_types.retain(|work| *work != DeferredWorkType::PerceptualHash);
    }
    work_types
}

fn compare_import_quality(
    existing: &crate::db::query::ingest::ExistingImportTarget,
    new_single: &IngestPreparedSingle,
) -> crate::duplicates::quality::ImageQualityDecision {
    crate::duplicates::quality::compare_static_image_quality(
        &crate::duplicates::quality::ComparableImageCandidate {
            mime_type: &existing.mime_type,
            size_bytes: existing.size_bytes,
            pixel_width: existing.pixel_width,
            pixel_height: existing.pixel_height,
            frame_count: existing.frame_count,
        },
        &crate::duplicates::quality::ComparableImageCandidate {
            mime_type: &new_single.mime_type,
            size_bytes: new_single.size_bytes,
            pixel_width: new_single.pixel_width,
            pixel_height: new_single.pixel_height,
            frame_count: new_single.frame_count,
        },
    )
}

fn upsert_duplicate_pairs_for_candidates(
    canonical_db: &LibraryDatabase,
    imported_file_id: i64,
    candidates: &[PerceptualHashCandidate],
) -> Result<(), String> {
    for candidate in candidates {
        canonical_db.upsert_duplicate_pair_for_review(imported_file_id, candidate.file_id, candidate.distance)?;
    }
    Ok(())
}

pub fn apply_compiler_plan(db: &LibraryDatabase, flags: &IngestFlags, folder_ids: &[i64]) {
    let mut plan = CompilerPlan::default();
    if flags.status_changed {
        plan.rebuild_status = true;
        plan.rebuild_sidebar = true;
    }
    if flags.tags_changed {
        plan.rebuild_all_smart_folders = true;
        plan.rebuild_sidebar = true;
    }
    if flags.metadata_changed {
        plan.rebuild_sidebar = true;
    }
    if !folder_ids.is_empty() {
        plan.rebuild_sidebar = true;
    }
    if !plan.is_empty() {
        db.run_compiler(plan);
    }
}

pub fn build_ingest_change_impact(
    summary: &IngestBatchSummary,
    extra_grid_scopes: Vec<String>,
) -> crate::runtime_contract::change_builder::ChangeImpact {
    let mut impact = crate::runtime_contract::change_builder::ChangeImpact::new()
        .entity_hashes(
            summary
                .imported_hashes
                .iter()
                .cloned()
                .chain(summary.skipped_hashes.iter().cloned())
                .collect(),
        )
        .extra_grid_scopes(extra_grid_scopes);
    if summary.flags.status_changed {
        impact = impact.status_changed().status_sensitive_grid_scopes_changed();
    }
    if summary.flags.tags_changed {
        impact = impact.tags_changed().all_smart_folder_scopes_changed();
    }
    if summary.flags.metadata_changed {
        impact = impact.media_metadata_changed();
    }
    if !summary.folder_ids.is_empty() {
        impact = impact
            .folder_ids(summary.folder_ids.clone())
            .folder_membership_changed(summary.folder_ids.clone());
    }
    impact
}

async fn merge_existing_import_target(
    canonical_db: &LibraryDatabase,
    legacy_db: Option<&SqliteDatabase>,
    existing: &crate::db::query::ingest::ExistingImportTarget,
    request: &SingleIngestRequest,
) -> Result<SingleIngestOutcome, String> {
    let mut flags = IngestFlags::default();

    if existing.status != request.initial_status {
        if existing.status == 2 && request.initial_status != 2 {
            canonical_db.set_entity_status(
                &[existing.entity_id],
                request.initial_status,
                crate::db::types::ExpansionMode::EntityOnly,
            )?;
            flags.status_changed = true;
        } else if existing.status != 2 {
            canonical_db.set_entity_status(
                &[existing.entity_id],
                request.initial_status,
                crate::db::types::ExpansionMode::EntityOnly,
            )?;
            flags.status_changed = true;
        }
    }

    let existing_tags = canonical_db.get_entity_tags(&existing.entity_hash)?;
    let existing_tag_set: HashSet<String> = existing_tags
        .into_iter()
        .map(|tag| normalize::combine_tag(&tag.namespace, &tag.subtag))
        .collect();
    let missing_tags: Vec<String> = request
        .tag_strings
        .iter()
        .filter(|tag| !existing_tag_set.contains(*tag))
        .cloned()
        .collect();
    if !missing_tags.is_empty() {
        canonical_db.add_tags(
            &[existing.entity_id],
            &missing_tags,
            request.tag_provenance_mask,
            crate::db::types::ExpansionMode::EntityOnly,
        )?;
        flags.tags_changed = true;
    }

    let merged_urls: Vec<String> = {
        let current: Vec<String> = existing
            .source_urls_json
            .as_deref()
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or_default();
        let mut merged = current.clone();
        let mut seen: HashSet<String> = current.into_iter().collect();
        for url in dedupe_urls(request.source_urls.clone()) {
            if seen.insert(url.clone()) {
                merged.push(url);
            }
        }
        merged
    };

    let merged_notes = merge_note_text(existing.notes.as_deref(), request.notes.as_deref());
    let name_change = request
        .name
        .as_deref()
        .filter(|name| existing.name.as_deref().unwrap_or("") != *name)
        .map(ToOwned::to_owned);
    if name_change.is_some()
        || merged_notes.as_deref() != existing.notes.as_deref()
        || serde_json::to_string(&merged_urls).ok().as_deref() != existing.source_urls_json.as_deref()
    {
        canonical_db.patch_entity_metadata(
            &[existing.entity_id],
            &MediaEntityPatch {
                name: name_change,
                notes: Some(merged_notes.clone()),
                rating: None,
                source_urls: Some(merged_urls.clone()),
            },
        )?;
        flags.metadata_changed = true;
    }

    if let Some(created_at) = request.created_at.as_deref() {
        if !created_at.trim().is_empty() && existing.date_created != created_at {
            canonical_db.set_entity_date_created(&existing.entity_hash, created_at)?;
            flags.metadata_changed = true;
        }
    }

    if let Some(subscription_id) = request.subscription_id {
        if let Some(legacy_db) = legacy_db {
            let _ = legacy_db
                .add_subscription_entity(subscription_id, &existing.entity_hash)
                .await;
        }
    }

    Ok(SingleIngestOutcome {
        entity_hash: existing.entity_hash.clone(),
        imported_new: false,
        mime: existing.mime_type.clone(),
        size: existing.size_bytes as u64,
        has_thumbnail: capabilities_for_stored_media(&existing.mime_type, existing.frame_count)
            .can_thumbnail(),
        tags_applied: missing_tags,
        flags,
        scheduled_work: 0,
        entity_id: Some(existing.entity_id),
        file_id: Some(existing.file_id),
    })
}

pub async fn ingest_single_path(
    canonical_db: &LibraryDatabase,
    legacy_db: Option<&SqliteDatabase>,
    blob_store: &BlobStore,
    request: &SingleIngestRequest,
) -> Result<SingleIngestOutcome, String> {
    let options = build_import_options(request);
    let prepared_blob = ImportPipeline::prepare_blob_import(blob_store, &request.path, &options)
        .await
        .map_err(|err| err.to_string())?;

    if let Some(existing) = canonical_db.get_existing_import_target_by_file_hash(&prepared_blob.hex_hash)? {
        return merge_existing_import_target(canonical_db, legacy_db, &existing, request).await;
    }

    let mut prepared_single = prepared_from_blob_import(prepared_blob.clone(), request);
    let imported_phash = compute_comparable_image_phash(
        &request.path,
        &prepared_blob.mime,
        prepared_blob.num_frames,
    )?;
    prepared_single.perceptual_hash = imported_phash.clone();

    let threshold = duplicate_review_distance_threshold();
    let candidates = if let Some(phash) = imported_phash.as_deref() {
        canonical_db.find_perceptual_hash_candidates(phash, threshold)?
    } else {
        Vec::new()
    };

    let exact_matches: Vec<PerceptualHashCandidate> = candidates
        .iter()
        .filter(|candidate| candidate.distance == 0)
        .cloned()
        .collect();
    let near_matches: Vec<PerceptualHashCandidate> = candidates
        .iter()
        .filter(|candidate| candidate.distance > 0)
        .cloned()
        .collect();

    if exact_matches.len() == 1 {
        let candidate = &exact_matches[0];
        if let Some(existing) =
            canonical_db.get_existing_import_target_by_file_hash(&candidate.file_hash)?
        {
            match compare_import_quality(&existing, &prepared_single) {
                crate::duplicates::quality::ImageQualityDecision::LeftBetter => {
                    return merge_existing_import_target(canonical_db, legacy_db, &existing, request)
                        .await;
                }
                crate::duplicates::quality::ImageQualityDecision::RightBetter => {}
                crate::duplicates::quality::ImageQualityDecision::Ambiguous => {}
            }
        }
    }

    let entity_id = canonical_db.insert_ingested_single(&prepared_single)?;
    let imported_target = canonical_db
        .get_existing_import_target_by_file_hash(&prepared_single.entity_hash)?
        .ok_or_else(|| "Inserted ingest target could not be reloaded".to_string())?;

    let work_types = work_types_for_new_ingest(
        &prepared_single.mime_type,
        prepared_single.frame_count,
        !prepared_blob.has_thumbnail && !request.skip_thumbnail,
        prepared_single.perceptual_hash.is_some(),
    );
    if !work_types.is_empty() {
        canonical_db.enqueue_deferred_jobs(&prepared_single.entity_hash, &work_types)?;
    }

    let mut final_entity_hash = prepared_single.entity_hash.clone();
    let mut final_entity_id = entity_id;
    let mut final_file_id = imported_target.file_id;
    let mut imported_new = true;

    if exact_matches.len() == 1 {
        let candidate = &exact_matches[0];
        if let Some(existing) =
            canonical_db.get_existing_import_target_by_file_hash(&candidate.file_hash)?
        {
            match compare_import_quality(&existing, &prepared_single) {
                crate::duplicates::quality::ImageQualityDecision::RightBetter => {
                    let resolution = canonical_db.resolve_duplicate_pair(
                        "smart_merge",
                        &existing.entity_hash,
                        &prepared_single.entity_hash,
                        None,
                    )?;
                    if let Some(loser_hash) = resolution.loser_hash.as_deref() {
                        let _ = blob_store.delete(loser_hash);
                    }
                    if let Some(winner_hash) = resolution.winner_hash {
                        final_entity_hash = winner_hash.clone();
                        if let Some(winner) =
                            canonical_db.get_existing_import_target_by_entity_hash(&winner_hash)?
                        {
                            final_entity_id = winner.entity_id;
                            final_file_id = winner.file_id;
                        }
                    }
                }
                crate::duplicates::quality::ImageQualityDecision::Ambiguous => {
                    upsert_duplicate_pairs_for_candidates(
                        canonical_db,
                        imported_target.file_id,
                        &exact_matches,
                    )?;
                }
                crate::duplicates::quality::ImageQualityDecision::LeftBetter => {
                    imported_new = false;
                }
            }
        }
    } else {
        if !exact_matches.is_empty() {
            upsert_duplicate_pairs_for_candidates(canonical_db, imported_target.file_id, &exact_matches)?;
        }
    }

    if !near_matches.is_empty() {
        upsert_duplicate_pairs_for_candidates(canonical_db, imported_target.file_id, &near_matches)?;
    }

    Ok(SingleIngestOutcome {
        entity_hash: final_entity_hash,
        imported_new,
        mime: prepared_blob.mime,
        size: prepared_blob.size,
        has_thumbnail: capabilities_for_stored_media(
            &prepared_single.mime_type,
            prepared_single.frame_count,
        )
        .can_thumbnail(),
        tags_applied: prepared_blob.tags_applied,
        flags: IngestFlags {
            status_changed: true,
            tags_changed: !prepared_single.tag_strings.is_empty(),
            metadata_changed: prepared_single.name.is_some()
                || prepared_single.notes.is_some()
                || !prepared_single.source_urls.is_empty(),
        },
        scheduled_work: work_types.len(),
        entity_id: Some(final_entity_id),
        file_id: Some(final_file_id),
    })
}

pub async fn import_files(
    canonical_db: &LibraryDatabase,
    legacy_db: Option<&SqliteDatabase>,
    blob_store: &BlobStore,
    paths: Vec<String>,
    tag_strings: Option<Vec<String>>,
    source_urls: Option<Vec<String>>,
    initial_status: i64,
    library_root: Option<&Path>,
) -> Result<(ImportBatchResult, IngestBatchSummary), String> {
    let tag_strings = normalize_manual_tags(&tag_strings.unwrap_or_default());
    let source_urls = dedupe_urls(source_urls.unwrap_or_default());
    let file_paths: Vec<PathBuf> = paths
        .into_iter()
        .flat_map(|raw| {
            let path = PathBuf::from(&raw);
            let path = path.canonicalize().unwrap_or(path);
            if path.is_dir() {
                collect_files_recursive(&path)
            } else {
                vec![path]
            }
        })
        .filter(|path| {
            path.is_file()
                && crate::media_processing::has_supported_extension(path)
                && !library_root.is_some_and(|root| path.starts_with(root))
        })
        .collect();

    let mut batch = ImportBatchResult {
        imported: Vec::new(),
        skipped: Vec::new(),
        errors: Vec::new(),
    };
    let mut summary = IngestBatchSummary::default();

    for path in file_paths {
        let request = SingleIngestRequest {
            source_kind: IngestSourceKind::Manual,
            path,
            tag_strings: tag_strings.clone(),
            source_urls: source_urls.clone(),
            name: None,
            notes: None,
            created_at: None,
            initial_status,
            skip_thumbnail: false,
            tag_provenance_mask: TAG_PROVENANCE_MANUAL,
            subscription_id: None,
        };
        match ingest_single_path(canonical_db, legacy_db, blob_store, &request).await {
            Ok(outcome) => {
                summary.flags.merge(&outcome.flags);
                summary.scheduled_work += outcome.scheduled_work;
                if outcome.imported_new {
                    summary.imported_hashes.push(outcome.entity_hash.clone());
                    batch.imported.push(ImportResult {
                        hash: outcome.entity_hash,
                        mime: outcome.mime,
                        size: outcome.size,
                        has_thumbnail: outcome.has_thumbnail,
                        tags_applied: outcome.tags_applied,
                    });
                } else {
                    summary.skipped_hashes.push(outcome.entity_hash.clone());
                    batch.skipped.push(outcome.entity_hash);
                }
            }
            Err(error) => batch.errors.push(error),
        }
    }

    Ok((batch, summary))
}

pub async fn import_folder(
    canonical_db: &LibraryDatabase,
    legacy_db: Option<&SqliteDatabase>,
    blob_store: &BlobStore,
    path: String,
    preserve_structure: bool,
    parent_folder_id: Option<i64>,
    initial_status: i64,
) -> Result<(ImportBatchResult, IngestBatchSummary), String> {
    let root_path = {
        let path = PathBuf::from(path);
        path.canonicalize().unwrap_or(path)
    };
    if !root_path.is_dir() {
        return Err(format!("Folder not found: {}", root_path.display()));
    }

    let (directories, file_paths) = collect_import_paths(&root_path)?;
    let mut batch = ImportBatchResult {
        imported: Vec::new(),
        skipped: Vec::new(),
        errors: Vec::new(),
    };
    let mut summary = IngestBatchSummary::default();

    let mut folder_cache = HashMap::<PathBuf, i64>::new();
    if preserve_structure {
        let root_name = root_path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("Imported Folder")
            .to_string();
        let root_folder_id = canonical_db.create_folder(&root_name, parent_folder_id, None, None)?;
        folder_cache.insert(PathBuf::new(), root_folder_id);
        summary.folder_ids.push(root_folder_id);

        for directory in directories {
            let relative = match directory.strip_prefix(&root_path) {
                Ok(relative) if !relative.as_os_str().is_empty() => relative.to_path_buf(),
                _ => continue,
            };
            let parent_relative = relative.parent().map(Path::to_path_buf).unwrap_or_default();
            let Some(parent_id) = folder_cache.get(&parent_relative).copied() else {
                continue;
            };
            let name = directory
                .file_name()
                .and_then(|entry| entry.to_str())
                .filter(|entry| !entry.is_empty())
                .unwrap_or("Imported Folder")
                .to_string();
            let folder_id = canonical_db.create_folder(&name, Some(parent_id), None, None)?;
            folder_cache.insert(relative, folder_id);
            summary.folder_ids.push(folder_id);
        }
    }

    for file_path in file_paths {
        let target_folder_id = if preserve_structure {
            let relative_parent = file_path
                .strip_prefix(&root_path)
                .ok()
                .and_then(|relative| relative.parent())
                .map(Path::to_path_buf)
                .unwrap_or_default();
            folder_cache.get(&relative_parent).copied()
        } else {
            parent_folder_id
        };

        let request = SingleIngestRequest {
            source_kind: IngestSourceKind::Manual,
            path: file_path,
            tag_strings: Vec::new(),
            source_urls: Vec::new(),
            name: None,
            notes: None,
            created_at: None,
            initial_status,
            skip_thumbnail: false,
            tag_provenance_mask: TAG_PROVENANCE_MANUAL,
            subscription_id: None,
        };
        match ingest_single_path(canonical_db, legacy_db, blob_store, &request).await {
            Ok(outcome) => {
                summary.flags.merge(&outcome.flags);
                summary.scheduled_work += outcome.scheduled_work;
                if let Some(folder_id) = target_folder_id {
                    let ids = canonical_db.resolve_entity_hashes(&[outcome.entity_hash.clone()])?;
                    if !ids.is_empty() {
                        canonical_db.add_folder_members(
                            folder_id,
                            &ids,
                            crate::db::types::ExpansionMode::EntityOnly,
                        )?;
                        if !summary.folder_ids.contains(&folder_id) {
                            summary.folder_ids.push(folder_id);
                        }
                    }
                }
                if outcome.imported_new {
                    summary.imported_hashes.push(outcome.entity_hash.clone());
                    batch.imported.push(ImportResult {
                        hash: outcome.entity_hash,
                        mime: outcome.mime,
                        size: outcome.size,
                        has_thumbnail: outcome.has_thumbnail,
                        tags_applied: outcome.tags_applied,
                    });
                } else {
                    summary.skipped_hashes.push(outcome.entity_hash.clone());
                    batch.skipped.push(outcome.entity_hash);
                }
            }
            Err(error) => batch.errors.push(error),
        }
    }

    summary.folder_ids.sort_unstable();
    summary.folder_ids.dedup();
    Ok((batch, summary))
}

pub async fn import_watch_path(
    canonical_db: &LibraryDatabase,
    legacy_db: Option<&SqliteDatabase>,
    blob_store: &BlobStore,
    root_folder_id: i64,
    root_path: &Path,
    watch_subfolders: bool,
    watch_import_status_mode: &str,
    path: &Path,
) -> Result<IngestBatchSummary, String> {
    let relative_parent = path
        .strip_prefix(root_path)
        .ok()
        .and_then(|relative| relative.parent())
        .unwrap_or_else(|| Path::new(""));
    if !watch_subfolders && !relative_parent.as_os_str().is_empty() {
        return Ok(IngestBatchSummary::default());
    }

    let target_folder_id = if relative_parent.as_os_str().is_empty() {
        root_folder_id
    } else {
        let mut current_folder_id = root_folder_id;
        for component in relative_parent.components() {
            let Component::Normal(name) = component else {
                continue;
            };
            let Some(name) = name.to_str() else {
                continue;
            };
            let child_id = match canonical_db.find_child_folder_id(current_folder_id, name)? {
                Some(folder_id) => folder_id,
                None => canonical_db.create_folder(name, Some(current_folder_id), None, None)?,
            };
            current_folder_id = child_id;
        }
        current_folder_id
    };

    let initial_status = match watch_import_status_mode {
        "inbox" => 0,
        "active" => 1,
        "inherit" => {
            let default_mode = crate::state::get_state()
                .map(|state| state.settings.get().watch_folder_default_status)
                .unwrap_or_else(|_| "inbox".to_string());
            if default_mode == "active" { 1 } else { 0 }
        }
        other => return Err(format!("Invalid watch import status mode: {other}")),
    };

    let request = SingleIngestRequest {
        source_kind: IngestSourceKind::WatchFolder,
        path: path.to_path_buf(),
        tag_strings: Vec::new(),
        source_urls: Vec::new(),
        name: None,
        notes: None,
        created_at: None,
        initial_status,
        skip_thumbnail: false,
        tag_provenance_mask: TAG_PROVENANCE_UNKNOWN,
        subscription_id: None,
    };
    let outcome = ingest_single_path(canonical_db, legacy_db, blob_store, &request).await?;
    let mut summary = IngestBatchSummary::default();
    summary.flags.merge(&outcome.flags);
    summary.scheduled_work += outcome.scheduled_work;
    summary.folder_ids.push(target_folder_id);
    let ids = canonical_db.resolve_entity_hashes(&[outcome.entity_hash.clone()])?;
    if !ids.is_empty() {
        canonical_db.add_folder_members(
            target_folder_id,
            &ids,
            crate::db::types::ExpansionMode::EntityOnly,
        )?;
    }
    if outcome.imported_new {
        summary.imported_hashes.push(outcome.entity_hash);
    } else {
        summary.skipped_hashes.push(outcome.entity_hash);
    }
    Ok(summary)
}

pub async fn ingest_subscription_item(
    canonical_db: &LibraryDatabase,
    legacy_db: &SqliteDatabase,
    blob_store: &BlobStore,
    file_path: &Path,
    metadata: &ParsedMetadata,
    subscription_id: i64,
    skip_thumbnail: bool,
    initial_status: i64,
) -> Result<SingleIngestOutcome, String> {
    let request = SingleIngestRequest {
        source_kind: IngestSourceKind::Subscription,
        path: file_path.to_path_buf(),
        tag_strings: normalize_subscription_tags(metadata),
        source_urls: dedupe_urls(metadata.source_urls.clone()),
        name: crate::subscriptions::import_policy::preferred_import_name(metadata),
        notes: metadata_notes_text(metadata),
        created_at: metadata.created_at.clone(),
        initial_status,
        skip_thumbnail,
        tag_provenance_mask: TAG_PROVENANCE_UNKNOWN,
        subscription_id: Some(subscription_id),
    };
    ingest_single_path(canonical_db, Some(legacy_db), blob_store, &request).await
}

pub async fn materialize_subscription_collection(
    canonical_db: &LibraryDatabase,
    legacy_db: &SqliteDatabase,
    blob_store: &BlobStore,
    subscription_id: i64,
    site_category: &str,
    post_id: &str,
    preferred_name: &str,
    members: &[SubscriptionCollectionMember],
) -> Result<SubscriptionCollectionOutcome, String> {
    let mut existing_member_ids = Vec::new();
    let mut new_members = Vec::new();
    let mut pending_review_pairs = Vec::<(String, Vec<PerceptualHashCandidate>)>::new();
    let mut pending_exact_upgrades = Vec::<(String, String)>::new();
    let mut imported_hashes = Vec::new();
    let mut flags = IngestFlags::default();
    let mut scheduled_work = 0usize;

    for member in members {
        let request = SingleIngestRequest {
            source_kind: IngestSourceKind::Subscription,
            path: member.path.clone(),
            tag_strings: normalize_subscription_tags(&member.metadata),
            source_urls: dedupe_urls(member.metadata.source_urls.clone()),
            name: crate::subscriptions::import_policy::preferred_import_name(&member.metadata),
            notes: metadata_notes_text(&member.metadata),
            created_at: member.metadata.created_at.clone(),
            initial_status: 0,
            skip_thumbnail: member.skip_thumbnail,
            tag_provenance_mask: TAG_PROVENANCE_UNKNOWN,
            subscription_id: Some(subscription_id),
        };
        let options = build_import_options(&request);
        let prepared_blob = ImportPipeline::prepare_blob_import(blob_store, &request.path, &options)
            .await
            .map_err(|err| err.to_string())?;
        if let Some(existing) = canonical_db.get_existing_import_target_by_file_hash(&prepared_blob.hex_hash)? {
            let merge = merge_existing_import_target(canonical_db, Some(legacy_db), &existing, &request).await?;
            flags.merge(&merge.flags);
            existing_member_ids.push(existing.entity_id);
            continue;
        }

        let mut prepared_single = prepared_from_blob_import(prepared_blob.clone(), &request);
        let imported_phash = compute_comparable_image_phash(
            &request.path,
            &prepared_blob.mime,
            prepared_blob.num_frames,
        )?;
        prepared_single.perceptual_hash = imported_phash.clone();

        let threshold = duplicate_review_distance_threshold();
        let candidates = if let Some(phash) = imported_phash.as_deref() {
            canonical_db.find_perceptual_hash_candidates(phash, threshold)?
        } else {
            Vec::new()
        };
        let exact_matches: Vec<PerceptualHashCandidate> = candidates
            .iter()
            .filter(|candidate| candidate.distance == 0)
            .cloned()
            .collect();
        let near_matches: Vec<PerceptualHashCandidate> = candidates
            .iter()
            .filter(|candidate| candidate.distance > 0)
            .cloned()
            .collect();

        if exact_matches.len() == 1 {
            let candidate = &exact_matches[0];
            if let Some(existing) =
                canonical_db.get_existing_import_target_by_file_hash(&candidate.file_hash)?
            {
                match compare_import_quality(&existing, &prepared_single) {
                    crate::duplicates::quality::ImageQualityDecision::LeftBetter => {
                        let merge = merge_existing_import_target(
                            canonical_db,
                            Some(legacy_db),
                            &existing,
                            &request,
                        )
                        .await?;
                        flags.merge(&merge.flags);
                        existing_member_ids.push(existing.entity_id);
                        continue;
                    }
                    crate::duplicates::quality::ImageQualityDecision::RightBetter => {
                        pending_exact_upgrades.push((
                            existing.entity_hash.clone(),
                            prepared_single.entity_hash.clone(),
                        ));
                    }
                    crate::duplicates::quality::ImageQualityDecision::Ambiguous => {
                        pending_review_pairs
                            .push((prepared_single.entity_hash.clone(), exact_matches.clone()));
                    }
                }
            }
        } else if !exact_matches.is_empty() {
            pending_review_pairs.push((prepared_single.entity_hash.clone(), exact_matches));
        }

        if !near_matches.is_empty() {
            pending_review_pairs.push((prepared_single.entity_hash.clone(), near_matches));
        }

        imported_hashes.push(prepared_single.entity_hash.clone());
        scheduled_work += work_types_for_new_ingest(
            &prepared_single.mime_type,
            prepared_single.frame_count,
            !prepared_blob.has_thumbnail && !request.skip_thumbnail,
            prepared_single.perceptual_hash.is_some(),
        )
        .len();
        new_members.push(prepared_single);
    }

    if new_members.is_empty() && existing_member_ids.len() < 2 {
        return Ok(SubscriptionCollectionOutcome {
            collection_id: None,
            collection_hash: None,
            imported_hashes,
            flags,
            scheduled_work,
        });
    }

    if new_members.len() + existing_member_ids.len() < 2 {
        if let Some(member) = new_members.into_iter().next() {
            canonical_db.insert_ingested_single(&member)?;
            let work_types = work_types_for_new_ingest(
                &member.mime_type,
                member.frame_count,
                !member.has_thumbnail && !member.skip_thumbnail,
                member.perceptual_hash.is_some(),
            );
            if !work_types.is_empty() {
                canonical_db.enqueue_deferred_jobs(&member.entity_hash, &work_types)?;
            }
        }
        return Ok(SubscriptionCollectionOutcome {
            collection_id: None,
            collection_hash: None,
            imported_hashes,
            flags: IngestFlags {
                status_changed: true,
                ..flags
            },
            scheduled_work,
        });
    }

    let (collection_id, collection_hash, new_hashes) =
        canonical_db.materialize_ingested_collection(preferred_name, &new_members, &existing_member_ids)?;

    let batch: Vec<(String, Vec<DeferredWorkType>)> = new_members
        .iter()
        .map(|member| {
            let work_types = work_types_for_new_ingest(
                &member.mime_type,
                member.frame_count,
                !member.has_thumbnail && !member.skip_thumbnail,
                member.perceptual_hash.is_some(),
            );
            (member.entity_hash.clone(), work_types)
        })
        .filter(|(_, work_types)| !work_types.is_empty())
        .collect();
    if !batch.is_empty() {
        canonical_db.enqueue_deferred_jobs_batch(batch)?;
    }

    let _ = legacy_db
        .upsert_subscription_post_collection(subscription_id, site_category, post_id, collection_id)
        .await;

    for (new_hash, candidates) in &pending_review_pairs {
        let Some(new_target) = canonical_db.get_existing_import_target_by_entity_hash(new_hash)? else {
            continue;
        };
        upsert_duplicate_pairs_for_candidates(canonical_db, new_target.file_id, candidates)?;
    }

    for (existing_hash, new_hash) in &pending_exact_upgrades {
        let resolution = canonical_db.resolve_duplicate_pair(
            "smart_merge",
            existing_hash,
            new_hash,
            Some(collection_id),
        )?;
        if let Some(loser_hash) = resolution.loser_hash.as_deref() {
            let _ = blob_store.delete(loser_hash);
        }
    }

    imported_hashes.extend(new_hashes);
    flags.status_changed = true;
    Ok(SubscriptionCollectionOutcome {
        collection_id: Some(collection_id),
        collection_hash: Some(collection_hash),
        imported_hashes,
        flags,
        scheduled_work,
    })
}
