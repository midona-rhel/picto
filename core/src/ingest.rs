use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::background_work::DeferredWorkType;
use crate::blob_store::BlobStore;
use crate::db::projection::compiler::CompilerPlan;
use crate::db::types::{IngestPreparedSingle, MediaEntityPatch};
use crate::db::LibraryDatabase;
use crate::import::pipeline::{ImportOptions, ImportPipeline};
use crate::media_analysis;
use crate::media_capabilities::capabilities_for_stored_media;
use crate::subscriptions::source_adapter::ParsedMetadata;
use crate::tags::normalize;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
    pub duplicates_changed: bool,
}

impl IngestFlags {
    pub(crate) fn merge(&mut self, other: &Self) {
        self.status_changed |= other.status_changed;
        self.tags_changed |= other.tags_changed;
        self.metadata_changed |= other.metadata_changed;
        self.duplicates_changed |= other.duplicates_changed;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub enum SingleIngestDisposition {
    Imported,
    Reused,
}

impl SingleIngestDisposition {
    pub fn result_kind(&self) -> &'static str {
        match self {
            Self::Imported => "imported",
            Self::Reused => "reused",
        }
    }

    pub fn is_imported(&self) -> bool {
        matches!(self, Self::Imported)
    }
}

#[derive(Debug, Clone)]
pub struct SingleIngestOutcome {
    pub entity_hash: String,
    pub file_hash: String,
    pub disposition: SingleIngestDisposition,
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

pub fn dedupe_urls(mut urls: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    urls.retain(|url| {
        let trimmed = url.trim();
        !trimmed.is_empty() && seen.insert(trimmed.to_string())
    });
    urls
}

pub fn metadata_notes_text(metadata: &ParsedMetadata) -> Option<String> {
    let mut sections = Vec::new();
    if let Some(title) = metadata
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
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

pub fn normalize_subscription_tags(metadata: &ParsedMetadata) -> Vec<String> {
    let mut tags: Vec<String> = metadata
        .tags
        .iter()
        .map(|(ns, st)| normalize::combine_tag(ns, st))
        .collect();
    if let Some(rating) = normalize_source_rating(metadata) {
        let rating_tag = normalize::combine_tag("rating", rating);
        if !tags.contains(&rating_tag) {
            tags.push(rating_tag);
        }
    }
    tags
}

fn normalize_source_rating(metadata: &ParsedMetadata) -> Option<&'static str> {
    let rating = metadata.rating.as_deref()?.trim().to_ascii_lowercase();
    let canonical = match rating.as_str() {
        "g" | "general" => "general",
        "s" => match metadata.category.as_deref() {
            Some("e621") => "safe",
            Some("danbooru" | "gelbooru") => "sensitive",
            _ => "s",
        },
        "safe" => "safe",
        "sensitive" => "sensitive",
        "q" | "questionable" => "questionable",
        "e" | "explicit" => "explicit",
        _ => return None,
    };
    Some(canonical)
}

fn build_import_options(request: &SingleIngestRequest) -> ImportOptions {
    let mut options = ImportOptions {
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
    if let Some(notes) = request
        .notes
        .as_deref()
        .filter(|text| !text.trim().is_empty())
    {
        let mut map = HashMap::new();
        map.insert("text".to_string(), notes.to_string());
        options.notes = Some(map);
    }
    options
}

fn prepared_from_blob_import(
    prepared: &crate::import::pipeline::PreparedBlobImport,
    request: &SingleIngestRequest,
) -> IngestPreparedSingle {
    IngestPreparedSingle {
        entity_hash: prepared.hex_hash.clone(),
        name: prepared.name.clone(),
        size_bytes: prepared.size as i64,
        mime_type: prepared.mime.clone(),
        pixel_width: prepared.pixel_width,
        pixel_height: prepared.pixel_height,
        duration_ms: prepared.duration_ms,
        frame_count: prepared.num_frames,
        has_audio: prepared.has_audio,
        status: request.initial_status,
        date_created: prepared
            .created_at
            .clone()
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
    file_bytes: &[u8],
    mime_type: &str,
    frame_count: Option<i64>,
) -> Result<Option<String>, String> {
    let capabilities = capabilities_for_stored_media(mime_type, frame_count);
    if !capabilities.can_perceptual_hash {
        return Ok(None);
    }
    crate::media_processing::compute_phash_base64(file_bytes)
        .map(Some)
        .map_err(|err| format!("Failed to compute perceptual hash: {err}"))
}

fn work_types_for_new_ingest(
    mime_type: &str,
    frame_count: Option<i64>,
    needs_thumbnail: bool,
    has_perceptual_hash: bool,
    auto_tag_on_import: bool,
) -> Vec<DeferredWorkType> {
    let mut work_types =
        media_analysis::derivative_work_types_for_target(mime_type, frame_count, needs_thumbnail);
    if has_perceptual_hash {
        work_types.retain(|work| *work != DeferredWorkType::PerceptualHash);
    }
    if auto_tag_on_import && mime_type.starts_with("image/") {
        work_types.push(DeferredWorkType::AiTag);
    }
    work_types
}

fn auto_tag_on_import_enabled() -> bool {
    crate::state::get_state()
        .map(|state| state.settings.get().ai_tagger_auto_on_import)
        .unwrap_or(false)
}

pub fn apply_compiler_plan(db: &LibraryDatabase, flags: &IngestFlags, folder_ids: &[i64]) {
    let mut plan = CompilerPlan::default();
    if flags.status_changed {
        plan.rebuild_status = true;
        plan.rebuild_sidebar = true;
    }
    if flags.tags_changed {
        plan.rebuild_all_tags = true;
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

/// Attach fresh system-scope counts so the sidebar numbers update the moment
/// an item lands. Without this the UI's refresh races the debounced compiler
/// (which emits no follow-up event) and the inbox count sticks until the next
/// unrelated event.
pub fn attach_current_sidebar_counts(
    db: &LibraryDatabase,
    impact: crate::runtime_contract::change_builder::ChangeImpact,
    duplicates_changed: bool,
) -> crate::runtime_contract::change_builder::ChangeImpact {
    match db.get_scope_counts() {
        Ok(counts) => impact.sidebar_counts(crate::runtime_contract::state_change::SidebarCounts {
            active: counts.active,
            inbox: counts.inbox,
            trash: counts.trash,
            uncategorized: counts.uncategorized,
            untagged: counts.untagged,
            duplicates: if duplicates_changed {
                db.get_duplicate_count().unwrap_or(-1)
            } else {
                -1
            },
        }),
        Err(_) => impact,
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
        // Only set the flag — don't merge all system scopes.
        // The caller already passed the authoritative extra_grid_scopes.
        impact = impact.status_changed();
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
    existing: &crate::db::query::ingest::ExistingImportTarget,
    request: &SingleIngestRequest,
) -> Result<SingleIngestOutcome, String> {
    let mut flags = IngestFlags::default();

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
        )?;
        flags.tags_changed = true;
    }

    let current_urls: Vec<String> = existing
        .source_urls_json
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default();
    let mut merged_urls = current_urls.clone();
    let mut seen_urls: HashSet<String> = current_urls.iter().cloned().collect();
    for url in dedupe_urls(request.source_urls.clone()) {
        if seen_urls.insert(url.clone()) {
            merged_urls.push(url);
        }
    }
    let urls_changed = merged_urls != current_urls;

    let name_change = existing
        .name
        .as_deref()
        .is_none_or(|name| name.trim().is_empty())
        .then(|| request.name.as_deref().map(str::trim))
        .flatten()
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned);
    let notes_change = existing
        .notes
        .as_deref()
        .is_none_or(|notes| notes.trim().is_empty())
        .then(|| request.notes.as_deref().map(str::trim))
        .flatten()
        .filter(|notes| !notes.is_empty())
        .map(|notes| Some(notes.to_owned()));
    if name_change.is_some() || notes_change.is_some() || urls_changed {
        canonical_db.patch_entity_metadata(
            &[existing.entity_id],
            &MediaEntityPatch {
                name: name_change,
                notes: notes_change,
                rating: None,
                source_urls: urls_changed.then_some(merged_urls),
            },
        )?;
        flags.metadata_changed = true;
    }

    Ok(SingleIngestOutcome {
        entity_hash: existing.entity_hash.clone(),
        file_hash: existing.file_hash.clone(),
        disposition: SingleIngestDisposition::Reused,
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
    blob_store: &BlobStore,
    request: &SingleIngestRequest,
) -> Result<SingleIngestOutcome, String> {
    let options = build_import_options(request);
    let prepared_blob = ImportPipeline::prepare_blob_import(&request.path, &options)
        .await
        .map_err(|err| err.to_string())?;

    let blob_lease = blob_store.acquire_hash_lease(&prepared_blob.hex_hash).await;
    ImportPipeline::persist_blob_import(blob_store, &prepared_blob)
        .map_err(|err| err.to_string())?;
    if let Some(existing) =
        canonical_db.get_existing_import_target_by_file_hash_write(&prepared_blob.hex_hash)?
    {
        drop(blob_lease);
        return merge_existing_import_target(canonical_db, &existing, request).await;
    }

    let mut prepared_single = prepared_from_blob_import(&prepared_blob, request);
    let imported_phash = compute_comparable_image_phash(
        &prepared_blob.file_bytes,
        &prepared_blob.mime,
        prepared_blob.num_frames,
    )?;
    prepared_single.perceptual_hash = imported_phash.clone();

    let threshold = duplicate_review_distance_threshold();
    let auto_tag_on_import = auto_tag_on_import_enabled();
    let work_types = work_types_for_new_ingest(
        &prepared_single.mime_type,
        prepared_single.frame_count,
        !prepared_blob.has_thumbnail && !request.skip_thumbnail,
        prepared_single.perceptual_hash.is_some(),
        auto_tag_on_import,
    );
    let (entity_id, duplicates_changed) = match canonical_db.insert_ingested_single_with_blob_lease(
        &prepared_single,
        &work_types,
        threshold,
        blob_lease,
    ) {
        Ok(result) => result,
        Err(error) => {
            if let Some(existing) = canonical_db
                .get_existing_import_target_by_file_hash_write(&prepared_single.entity_hash)?
            {
                return merge_existing_import_target(canonical_db, &existing, request).await;
            }
            return Err(error);
        }
    };
    let imported_target = canonical_db
        .get_existing_import_target_by_file_hash(&prepared_single.entity_hash)?
        .ok_or_else(|| "Inserted ingest target could not be reloaded".to_string())?;

    Ok(SingleIngestOutcome {
        entity_hash: prepared_single.entity_hash.clone(),
        file_hash: prepared_single.entity_hash.clone(),
        disposition: SingleIngestDisposition::Imported,
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
            duplicates_changed,
        },
        scheduled_work: work_types.len(),
        entity_id: Some(entity_id),
        file_id: Some(imported_target.file_id),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        attach_current_sidebar_counts, ingest_single_path, normalize_subscription_tags,
        work_types_for_new_ingest, IngestSourceKind, SingleIngestDisposition, SingleIngestRequest,
    };
    use crate::background_work::DeferredWorkType;
    use crate::blob_store::BlobStore;
    use crate::db::types::MediaEntityPatch;
    use crate::db::LibraryDatabase;
    use crate::duplicates::phash::DEFAULT_DISTANCE_THRESHOLD;
    use crate::media_processing::compute_phash_base64;
    use crate::subscriptions::source_adapter::ParsedMetadata;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
    use img_hash::ImageHash;
    use std::fs;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn automatic_tag_work_is_only_queued_for_enabled_image_imports() {
        let enabled_image = work_types_for_new_ingest("image/png", None, false, true, true);
        assert!(enabled_image.contains(&DeferredWorkType::AiTag));

        let disabled_image = work_types_for_new_ingest("image/png", None, false, true, false);
        assert!(!disabled_image.contains(&DeferredWorkType::AiTag));

        let video = work_types_for_new_ingest("video/mp4", None, false, false, true);
        assert!(!video.contains(&DeferredWorkType::AiTag));
    }

    #[test]
    fn normalize_subscription_tags_preserves_literal_colons() {
        let metadata = ParsedMetadata {
            tags: vec![
                ("".to_string(), "http://example.com".to_string()),
                ("".to_string(), "dragon:quest".to_string()),
                ("creator".to_string(), "foo_artist".to_string()),
            ],
            ..Default::default()
        };

        let normalized = normalize_subscription_tags(&metadata);
        assert!(normalized
            .iter()
            .any(|tag| tag == "general:http://example.com"));
        assert!(normalized.iter().any(|tag| tag == "general:dragon:quest"));
        assert!(normalized.iter().any(|tag| tag == "creator:foo_artist"));
    }

    #[test]
    fn normalize_subscription_tags_preserves_source_rating_as_a_tag() {
        for (category, rating, expected) in [
            ("e621", "s", "rating:safe"),
            ("danbooru", "s", "rating:sensitive"),
            ("gelbooru", "q", "rating:questionable"),
        ] {
            let metadata = ParsedMetadata {
                rating: Some(rating.to_string()),
                category: Some(category.to_string()),
                ..Default::default()
            };
            assert_eq!(normalize_subscription_tags(&metadata), vec![expected]);
        }

        let existing = ParsedMetadata {
            tags: vec![("rating".to_string(), "explicit".to_string())],
            rating: Some("e".to_string()),
            ..Default::default()
        };
        assert_eq!(
            normalize_subscription_tags(&existing),
            vec!["rating:explicit"]
        );
    }

    fn open_test_library() -> (TempDir, LibraryDatabase, BlobStore, PathBuf) {
        let tmp = TempDir::new().expect("tempdir");
        let library_root = tmp.path().join("library");
        fs::create_dir_all(&library_root).expect("create library root");
        let source_root = tmp.path().join("source");
        fs::create_dir_all(&source_root).expect("create source root");
        let db = LibraryDatabase::open(&library_root).expect("open library db");
        let blob_store = BlobStore::open(&library_root).expect("open blob store");
        (tmp, db, blob_store, source_root)
    }

    fn request_for_path(path: &Path) -> SingleIngestRequest {
        SingleIngestRequest {
            source_kind: IngestSourceKind::Manual,
            path: path.to_path_buf(),
            tag_strings: Vec::new(),
            source_urls: Vec::new(),
            name: None,
            notes: None,
            created_at: None,
            initial_status: 1,
            skip_thumbnail: false,
            tag_provenance_mask: 0,
            subscription_id: None,
        }
    }

    fn encode_image(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), format)
            .expect("encode image");
        bytes
    }

    fn write_image(path: &Path, image: &DynamicImage, format: ImageFormat) {
        fs::write(path, encode_image(image, format)).expect("write image");
    }

    fn write_video(path: &Path) {
        let ffmpeg = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("vendor/ffmpeg/ffmpeg");
        let ffmpeg = if ffmpeg.exists() {
            ffmpeg
        } else {
            PathBuf::from("ffmpeg")
        };
        let status = std::process::Command::new(ffmpeg)
            .args([
                "-v",
                "quiet",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=0.2:size=32x32:rate=5",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path)
            .status()
            .expect("run ffmpeg fixture generator");
        assert!(status.success(), "generate video fixture");
    }

    fn solid_image(width: u32, height: u32, value: u8) -> DynamicImage {
        DynamicImage::ImageRgba8(ImageBuffer::from_pixel(
            width,
            height,
            Rgba([value, value, value, 255]),
        ))
    }

    fn patterned_image(width: u32, height: u32) -> DynamicImage {
        let mut buffer = ImageBuffer::new(width, height);
        for (x, y, pixel) in buffer.enumerate_pixels_mut() {
            let checker = ((x / 8) + (y / 8)) % 2 == 0;
            let base: u8 = if checker { 220 } else { 40 };
            let tint = ((x + y) % 17) as u8;
            *pixel = Rgba([
                base,
                base.saturating_sub(tint / 2),
                base.saturating_add(tint / 3),
                255,
            ]);
        }
        DynamicImage::ImageRgba8(buffer)
    }

    fn build_near_duplicate_variant(base: &DynamicImage) -> DynamicImage {
        let base_rgba = base.to_rgba8();
        let base_hash = ImageHash::<Vec<u8>>::from_base64(
            &compute_phash_base64(&encode_image(base, ImageFormat::Png)).expect("base phash"),
        )
        .expect("parse base hash");

        let mut variants = Vec::new();
        for shift in [1_u32, 2, 3, 4] {
            let mut shifted = ImageBuffer::new(base_rgba.width(), base_rgba.height());
            for x in 0..base_rgba.width() {
                for y in 0..base_rgba.height() {
                    let src_x = (x + shift) % base_rgba.width();
                    let src_y = (y + shift) % base_rgba.height();
                    *shifted.get_pixel_mut(x, y) = *base_rgba.get_pixel(src_x, src_y);
                }
            }
            variants.push(DynamicImage::ImageRgba8(shifted));
        }

        let anchor_points = [
            (0_u32, 0_u32),
            (base_rgba.width() / 4, base_rgba.height() / 4),
            (base_rgba.width() / 2, base_rgba.height() / 2),
            (
                base_rgba.width().saturating_sub(base_rgba.width() / 3),
                base_rgba.height() / 3,
            ),
        ];
        let deltas = [
            (-40_i16, 20_i16, -15_i16),
            (35_i16, -10_i16, 15_i16),
            (-25_i16, -25_i16, 30_i16),
        ];
        for block in [6_u32, 10, 14, 18, 24, 32, 40] {
            for &(x0, y0) in &anchor_points {
                for &(dr, dg, db) in &deltas {
                    let mut candidate = base_rgba.clone();
                    let x_end = (x0 + block).min(candidate.width());
                    let y_end = (y0 + block).min(candidate.height());
                    for x in x0..x_end {
                        for y in y0..y_end {
                            let pixel = candidate.get_pixel_mut(x, y);
                            let [r, g, b, a] = pixel.0;
                            *pixel = Rgba([
                                (r as i16 + dr).clamp(0, 255) as u8,
                                (g as i16 + dg).clamp(0, 255) as u8,
                                (b as i16 + db).clamp(0, 255) as u8,
                                a,
                            ]);
                        }
                    }
                    variants.push(DynamicImage::ImageRgba8(candidate));
                }
            }
        }

        for band in [8_u32, 12, 16, 24, 32] {
            let mut horizontal = base_rgba.clone();
            for y in 0..band.min(horizontal.height()) {
                for x in 0..horizontal.width() {
                    let pixel = horizontal.get_pixel_mut(x, y);
                    let [r, g, b, a] = pixel.0;
                    *pixel = Rgba([r.saturating_sub(45), g.saturating_add(10), b, a]);
                }
            }
            variants.push(DynamicImage::ImageRgba8(horizontal));

            let mut vertical = base_rgba.clone();
            for x in 0..band.min(vertical.width()) {
                for y in 0..vertical.height() {
                    let pixel = vertical.get_pixel_mut(x, y);
                    let [r, g, b, a] = pixel.0;
                    *pixel = Rgba([r, g.saturating_sub(35), b.saturating_add(20), a]);
                }
            }
            variants.push(DynamicImage::ImageRgba8(vertical));
        }

        for candidate_image in variants {
            let candidate_hash = ImageHash::<Vec<u8>>::from_base64(
                &compute_phash_base64(&encode_image(&candidate_image, ImageFormat::Png))
                    .expect("candidate phash"),
            )
            .expect("parse candidate hash");
            let distance = base_hash.dist(&candidate_hash);
            if distance > 0 && distance <= DEFAULT_DISTANCE_THRESHOLD {
                return candidate_image;
            }
        }
        panic!("failed to produce deterministic near-duplicate variant");
    }

    #[tokio::test]
    async fn image_and_video_import_as_two_direct_media_entities() {
        let (_tmp, db, blob_store, source_root) = open_test_library();
        let image_path = source_root.join("image.png");
        let video_path = source_root.join("video.mp4");
        write_image(&image_path, &solid_image(32, 32, 96), ImageFormat::Png);
        write_video(&video_path);

        let image = ingest_single_path(&db, &blob_store, &request_for_path(&image_path))
            .await
            .expect("ingest image");
        let video = ingest_single_path(&db, &blob_store, &request_for_path(&video_path))
            .await
            .expect("ingest video");

        assert!(image.disposition.is_imported());
        assert!(video.disposition.is_imported());
        assert!(image.mime.starts_with("image/"));
        assert!(video.mime.starts_with("video/"));
        assert_ne!(image.entity_hash, video.entity_hash);

        db.with_read(|conn| {
            let identities: Vec<(String, String)> = conn
                .prepare(
                    "SELECT me.entity_hash, mf.mime_type
                       FROM media_entity me
                       JOIN media_file mf ON mf.file_id = me.file_id
                      ORDER BY mf.mime_type",
                )?
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            assert_eq!(identities.len(), 2);
            assert!(identities
                .iter()
                .any(|(_, mime)| mime.starts_with("image/")));
            assert!(identities
                .iter()
                .any(|(_, mime)| mime.starts_with("video/")));
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row
                    .get::<_, i64>(0))?,
                2
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM media_file", [], |row| row
                    .get::<_, i64>(0))?,
                2
            );
            Ok(())
        })
        .unwrap();
    }

    #[tokio::test]
    async fn ingest_single_reuses_existing_entity_for_exact_file_hash() {
        let (_tmp, db, blob_store, source_root) = open_test_library();
        let source_path = source_root.join("exact.png");
        write_image(&source_path, &patterned_image(96, 96), ImageFormat::Png);

        let first = ingest_single_path(&db, &blob_store, &request_for_path(&source_path))
            .await
            .expect("first ingest");
        let original_path = blob_store
            .find_original(&first.file_hash, Some("png"))
            .expect("find original")
            .expect("original exists")
            .0;
        fs::remove_file(original_path).expect("remove original for repair test");
        let thumbnail_path = blob_store
            .find_thumbnail_path(&first.file_hash)
            .expect("find thumbnail")
            .expect("thumbnail exists");
        fs::remove_file(thumbnail_path).expect("remove thumbnail for repair test");

        let second = ingest_single_path(&db, &blob_store, &request_for_path(&source_path))
            .await
            .expect("second ingest");

        assert!(first.disposition.is_imported());
        assert!(matches!(
            second.disposition,
            SingleIngestDisposition::Reused
        ));
        assert_eq!(first.entity_hash, second.entity_hash);
        assert!(!second.flags.tags_changed);
        assert!(!second.flags.metadata_changed);
        assert!(!second.flags.status_changed);
        assert!(blob_store
            .read_original(&first.file_hash, Some("png"))
            .is_ok());
        assert!(blob_store
            .read_thumbnail(&first.file_hash)
            .expect("read repaired thumbnail")
            .is_some());
        db.with_read(|conn| {
            let entity_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row.get(0))?;
            let file_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM media_file", [], |row| row.get(0))?;
            assert_eq!(entity_count, 1);
            assert_eq!(file_count, 1);
            let source_urls_json: Option<String> = conn.query_row(
                "SELECT source_urls_json FROM media_entity WHERE entity_id = ?1",
                [first.entity_id.expect("inserted entity id")],
                |row| row.get(0),
            )?;
            assert!(source_urls_json.is_none());
            Ok(())
        })
        .expect("inspect exact hash reuse");
    }

    #[tokio::test]
    async fn reimport_only_enriches_reused_entity_metadata() {
        let (_tmp, db, blob_store, source_root) = open_test_library();
        let source_path = source_root.join("reimport.png");
        write_image(&source_path, &patterned_image(96, 96), ImageFormat::Png);

        let first = ingest_single_path(&db, &blob_store, &request_for_path(&source_path))
            .await
            .expect("initial ingest");
        let entity_id = first.entity_id.expect("inserted entity id");
        db.patch_entity_metadata(
            &[entity_id],
            &MediaEntityPatch {
                name: Some("existing name".to_string()),
                notes: Some(Some("existing notes".to_string())),
                rating: Some(4),
                source_urls: Some(vec!["https://existing.example/post".to_string()]),
            },
        )
        .expect("seed existing metadata");
        db.set_entity_status(&[entity_id], 2)
            .expect("seed existing status");
        db.set_entity_date_created(&first.entity_hash, "2001-02-03T04:05:06Z")
            .expect("seed existing creation date");

        let mut reimport = request_for_path(&source_path);
        reimport.initial_status = 1;
        reimport.tag_strings = vec![
            "general:incoming".to_string(),
            "rating:questionable".to_string(),
        ];
        reimport.source_urls = vec![
            "https://existing.example/post".to_string(),
            "https://incoming.example/post".to_string(),
            "https://incoming.example/post".to_string(),
        ];
        reimport.name = Some("incoming name".to_string());
        reimport.notes = Some("incoming notes".to_string());
        reimport.created_at = Some("2024-05-06T07:08:09Z".to_string());

        let reused = ingest_single_path(&db, &blob_store, &reimport)
            .await
            .expect("reimport");
        assert!(matches!(
            reused.disposition,
            SingleIngestDisposition::Reused
        ));
        assert_eq!(reused.entity_hash, first.entity_hash);
        assert!(reused.flags.tags_changed);
        assert!(reused.flags.metadata_changed);
        assert!(!reused.flags.status_changed);

        let tags = db.get_entity_tags(&first.entity_hash).expect("read tags");
        assert!(tags
            .iter()
            .any(|tag| tag.namespace == "general" && tag.subtag == "incoming"));
        assert!(tags
            .iter()
            .any(|tag| tag.namespace == "rating" && tag.subtag == "questionable"));
        db.with_read(|conn| {
            let (status, name, notes, rating, urls_json, date_created): (
                i64,
                Option<String>,
                Option<String>,
                Option<i64>,
                Option<String>,
                String,
            ) = conn.query_row(
                "SELECT status, name, notes, rating, source_urls_json, date_created
                 FROM media_entity WHERE entity_id = ?1",
                [entity_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )?;
            assert_eq!(status, 2);
            assert_eq!(name.as_deref(), Some("existing name"));
            assert_eq!(notes.as_deref(), Some("existing notes"));
            assert_eq!(rating, Some(4));
            assert_eq!(date_created, "2001-02-03T04:05:06Z");
            let urls: Vec<String> =
                serde_json::from_str(&urls_json.expect("source URLs")).expect("valid source URLs");
            assert_eq!(
                urls,
                vec![
                    "https://existing.example/post".to_string(),
                    "https://incoming.example/post".to_string(),
                ]
            );
            Ok(())
        })
        .expect("verify reimport contract");
    }

    #[tokio::test]
    async fn ingest_single_keeps_both_exact_phash_files_for_review() {
        let (_tmp, db, blob_store, source_root) = open_test_library();
        let image = patterned_image(96, 96);
        let jpeg_path = source_root.join("existing.jpg");
        let png_path = source_root.join("better.png");
        write_image(&jpeg_path, &image, ImageFormat::Jpeg);
        write_image(&png_path, &image, ImageFormat::Png);

        let jpeg_bytes = fs::read(&jpeg_path).expect("read jpeg");
        let png_bytes = fs::read(&png_path).expect("read png");
        let jpeg_phash = compute_phash_base64(&jpeg_bytes).expect("jpeg phash");
        let png_phash = compute_phash_base64(&png_bytes).expect("png phash");
        assert_eq!(jpeg_phash, png_phash);

        let first = ingest_single_path(&db, &blob_store, &request_for_path(&jpeg_path))
            .await
            .expect("ingest jpeg");
        let second = ingest_single_path(&db, &blob_store, &request_for_path(&png_path))
            .await
            .expect("ingest png");

        assert!(first.disposition.is_imported());
        assert!(second.disposition.is_imported());
        assert_eq!(db.get_duplicate_count().expect("duplicate count"), 1);
        let impact = attach_current_sidebar_counts(
            &db,
            crate::runtime_contract::change_builder::ChangeImpact::new(),
            true,
        );
        assert_eq!(impact.sidebar_counts.expect("sidebar counts").duplicates, 1);
        assert!(db
            .get_existing_import_target_by_file_hash(&first.file_hash)
            .expect("old target")
            .is_some());
        assert!(db
            .get_existing_import_target_by_file_hash(&second.file_hash)
            .expect("new target")
            .is_some());
        db.with_read(|conn| {
            let entity_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row.get(0))?;
            let file_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM media_file", [], |row| row.get(0))?;
            assert_eq!(entity_count, 2);
            assert_eq!(file_count, 2);
            Ok(())
        })
        .expect("inspect exact phash review");
    }

    #[tokio::test]
    async fn ingest_single_exact_phash_ambiguous_creates_duplicate_review_pair() {
        let (_tmp, db, blob_store, source_root) = open_test_library();
        let first_image = solid_image(100, 100, 180);
        let second_image = solid_image(105, 105, 180);
        let first_path = source_root.join("ambiguous_a.png");
        let second_path = source_root.join("ambiguous_b.png");
        write_image(&first_path, &first_image, ImageFormat::Png);
        write_image(&second_path, &second_image, ImageFormat::Png);

        let first_phash =
            compute_phash_base64(&fs::read(&first_path).expect("read first")).expect("first phash");
        let second_phash = compute_phash_base64(&fs::read(&second_path).expect("read second"))
            .expect("second phash");
        assert_eq!(first_phash, second_phash);

        let first = ingest_single_path(&db, &blob_store, &request_for_path(&first_path))
            .await
            .expect("ingest first ambiguous image");
        let second = ingest_single_path(&db, &blob_store, &request_for_path(&second_path))
            .await
            .expect("ingest second ambiguous image");

        assert!(first.disposition.is_imported());
        assert!(second.disposition.is_imported());
        db.with_read(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM duplicate WHERE status = 'detected'",
                [],
                |row| row.get(0),
            )?;
            let entity_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |row| row.get(0))?;
            assert_eq!(count, 1);
            assert_eq!(entity_count, 2);
            Ok(())
        })
        .expect("inspect ambiguous exact phash review");
    }

    #[tokio::test]
    async fn ingest_single_near_phash_creates_duplicate_review_pair() {
        let (_tmp, db, blob_store, source_root) = open_test_library();
        let base = patterned_image(96, 96);
        let near = build_near_duplicate_variant(&base);
        let first_path = source_root.join("near_a.png");
        let second_path = source_root.join("near_b.png");
        write_image(&first_path, &base, ImageFormat::Png);
        write_image(&second_path, &near, ImageFormat::Png);

        let first_hash = ImageHash::<Vec<u8>>::from_base64(
            &compute_phash_base64(&fs::read(&first_path).expect("read first"))
                .expect("first phash"),
        )
        .expect("parse first phash");
        let second_hash = ImageHash::<Vec<u8>>::from_base64(
            &compute_phash_base64(&fs::read(&second_path).expect("read second"))
                .expect("second phash"),
        )
        .expect("parse second phash");
        let distance = first_hash.dist(&second_hash);
        assert!(distance > 0);
        assert!(distance <= DEFAULT_DISTANCE_THRESHOLD);

        let first = ingest_single_path(&db, &blob_store, &request_for_path(&first_path))
            .await
            .expect("ingest first near image");
        let second = ingest_single_path(&db, &blob_store, &request_for_path(&second_path))
            .await
            .expect("ingest second near image");

        assert!(first.disposition.is_imported());
        assert!(second.disposition.is_imported());
        db.with_read(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM duplicate WHERE status = 'detected'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1);
            Ok(())
        })
        .expect("inspect near phash review");
    }

    #[tokio::test]
    async fn concurrent_near_phash_ingests_cannot_miss_the_review_pair() {
        let (_tmp, db, blob_store, source_root) = open_test_library();
        let base = patterned_image(96, 96);
        let near = build_near_duplicate_variant(&base);
        let first_path = source_root.join("concurrent_near_a.png");
        let second_path = source_root.join("concurrent_near_b.png");
        write_image(&first_path, &base, ImageFormat::Png);
        write_image(&second_path, &near, ImageFormat::Png);

        let db = Arc::new(db);
        let blob_store = Arc::new(blob_store);
        let first_request = request_for_path(&first_path);
        let second_request = request_for_path(&second_path);
        let (first, second) = tokio::join!(
            ingest_single_path(&db, &blob_store, &first_request),
            ingest_single_path(&db, &blob_store, &second_request),
        );
        assert!(first
            .expect("first concurrent ingest")
            .disposition
            .is_imported());
        assert!(second
            .expect("second concurrent ingest")
            .disposition
            .is_imported());
        assert_eq!(db.get_duplicate_count().expect("duplicate count"), 1);
    }
}
