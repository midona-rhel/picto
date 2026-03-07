//! Typed command implementations for media I/O operations: path resolution,
//! OS integration (open, reveal, export), thumbnails, blurhash backfill,
//! and color search.

use serde::Deserialize;
use ts_rs::TS;

use crate::blob_store::mime_to_extension;
use crate::state::AppState;
use super::{TypedCommand, run_typed};

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ResolveFilePathInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct OpenFileDefaultInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct RevealInFolderInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ExportFileInput {
    pub hash: String,
    pub dest_path: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct OpenInNewWindowInput {
    pub hash: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ResolveThumbnailPathInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct EnsureThumbnailInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct RegenerateThumbnailInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct RegenerateThumbnailsBatchInput {
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ReanalyzeFileColorsInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct BackfillMissingBlurhashesInput {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct SearchByColorInput {
    pub hex_color: String,
    pub max_distance: Option<f64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetImageThumbnailInput {
    #[serde(alias = "imageId")]
    pub hash: String,
}

// ─── Private result structs ────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
struct EnsureThumbnailResult {
    regenerated_thumbnail: bool,
    generated_blurhash: bool,
    has_thumbnail: bool,
    blurhash: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct BackfillMissingBlurhashesResult {
    processed: usize,
    regenerated_thumbnails: usize,
    generated_blurhashes: usize,
    remaining: usize,
}

#[derive(Debug, serde::Serialize)]
struct ReanalyzeFileColorsResult {
    colors_extracted: usize,
    dominant_color_hex: Option<String>,
}

// ─── Command structs ───────────────────────────────────────────────────────

pub struct ResolveFilePath;
pub struct OpenFileDefault;
pub struct RevealInFolder;
pub struct ExportFile;
pub struct OpenInNewWindow;
pub struct ResolveThumbnailPath;
pub struct EnsureThumbnail;
pub struct RegenerateThumbnail;
pub struct RegenerateThumbnailsBatch;
pub struct ReanalyzeFileColors;
pub struct BackfillMissingBlurhashes;
pub struct SearchByColor;
pub struct GetImageThumbnail;

// ─── Helper functions ──────────────────────────────────────────────────────

async fn resolve_file_path_inner(
    db: &crate::sqlite::SqliteDatabase,
    blob_store: &std::sync::Arc<crate::blob_store::BlobStore>,
    hash: &str,
) -> Result<String, String> {
    let file = db
        .get_file_by_hash(hash)
        .await?
        .ok_or_else(|| format!("File not found in database: {}", hash))?;
    let ext = mime_to_extension(&file.mime).to_string();
    let bs = blob_store.clone();
    let h = hash.to_string();
    tokio::task::spawn_blocking(move || {
        bs.find_original(&h, Some(&ext))
            .map_err(|e| format!("Blob error: {}", e))?
            .map(|(path, _)| path.to_string_lossy().into_owned())
            .ok_or_else(|| format!("File not found in blob store: {}", h))
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?
}

fn reveal_in_folder_inner(path: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to reveal in Finder: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path))
            .spawn()
            .map_err(|e| format!("Failed to reveal in Explorer: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::process::Command::new("xdg-open")
                .arg(parent)
                .spawn()
                .map_err(|e| format!("Failed to open folder: {}", e))?;
        }
    }

    Ok(())
}

async fn export_file_inner(
    db: &crate::sqlite::SqliteDatabase,
    blob_store: &std::sync::Arc<crate::blob_store::BlobStore>,
    hash: &str,
    dest_path: &str,
) -> Result<(), String> {
    let file = db
        .get_file_by_hash(hash)
        .await?
        .ok_or_else(|| format!("File not found in database: {}", hash))?;
    let ext = mime_to_extension(&file.mime).to_string();
    let dest = std::path::Path::new(dest_path);
    let parent = dest
        .parent()
        .ok_or_else(|| "Invalid destination path: no parent directory".to_string())?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| format!("Destination directory does not exist: {}", e))?;

    let blocked_prefixes = [
        "/etc",
        "/usr",
        "/bin",
        "/sbin",
        "/lib",
        "/var",
        "/sys",
        "/proc",
        "/dev",
        "C:\\Windows",
        "C:\\Program Files",
    ];
    let parent_str = canonical_parent.to_string_lossy();
    for prefix in &blocked_prefixes {
        if parent_str.starts_with(prefix) {
            return Err(format!(
                "Export to system directory '{}' is not allowed",
                prefix
            ));
        }
    }

    let blob_ref = blob_store.clone();
    let hash_clone = hash.to_string();
    let dest = dest.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let data = blob_ref
            .read_original(&hash_clone, Some(&ext))
            .map_err(|e| format!("Failed to read blob: {}", e))?;
        std::fs::write(&dest, &data).map_err(|e| format!("Failed to write export: {}", e))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("Export task failed: {}", e))??;

    Ok(())
}

async fn ensure_thumbnail_inner(
    db: &crate::sqlite::SqliteDatabase,
    blob_store: &std::sync::Arc<crate::blob_store::BlobStore>,
    hash: &str,
) -> Result<EnsureThumbnailResult, String> {
    generate_thumbnail_inner(db, blob_store, hash, false).await
}

/// Core thumbnail generation. When `force` is true, deletes existing thumbnail
/// first (used by regenerate). When false, skips if thumbnail already exists.
async fn generate_thumbnail_inner(
    db: &crate::sqlite::SqliteDatabase,
    blob_store: &std::sync::Arc<crate::blob_store::BlobStore>,
    hash: &str,
    force: bool,
) -> Result<EnsureThumbnailResult, String> {
    let file = db
        .get_file_by_hash(hash)
        .await?
        .ok_or_else(|| format!("File not found in database: {}", hash))?;

    let current_blurhash = file.blurhash.clone();
    let ext = mime_to_extension(&file.mime).to_string();
    let h = hash.to_string();
    let bs = blob_store.clone();
    let need_blurhash = current_blurhash.is_none() || force;

    let (regenerated_thumbnail, has_thumbnail, thumb_for_blurhash) =
        tokio::task::spawn_blocking(move || -> Result<(bool, bool, Option<Vec<u8>>), String> {
            if force {
                bs.delete_thumbnail(&h)
                    .map_err(|e| format!("Delete thumbnail failed: {}", e))?;
            }

            let original = bs
                .find_original(&h, Some(&ext))
                .map_err(|e| format!("Blob error: {}", e))?
                .ok_or_else(|| format!("Original file not found for hash {}", h))?;

            let mut thumb_bytes_for_blurhash: Option<Vec<u8>> = None;

            if !force {
                let thumb_exists = bs
                    .find_thumbnail_path(&h)
                    .map_err(|e| format!("Thumbnail lookup failed: {}", e))?
                    .is_some();

                if thumb_exists {
                    if need_blurhash {
                        thumb_bytes_for_blurhash = bs
                            .read_thumbnail(&h)
                            .map_err(|e| format!("Thumbnail read failed: {}", e))?;
                    }
                    return Ok((false, true, thumb_bytes_for_blurhash));
                }
            }

            let info = crate::media_processing::get_file_info(&original.0, None)
                .map_err(|e| format!("File info failed: {}", e))?;
            let (thumb_bytes, thumb_ext) = crate::media_processing::generate_thumbnail_bytes(
                &original.0,
                crate::media_processing::DEFAULT_THUMBNAIL_DIMENSIONS,
                info.mime,
                info.duration_ms,
                info.num_frames,
                35,
            )
            .map_err(|e| format!("Thumbnail generation failed: {}", e))?;

            bs.write_thumbnail(&h, &thumb_bytes, &thumb_ext)
                .map_err(|e| format!("Thumbnail write failed: {}", e))?;

            if need_blurhash {
                thumb_bytes_for_blurhash = Some(thumb_bytes);
            }

            Ok((true, true, thumb_bytes_for_blurhash))
        })
        .await
        .map_err(|e| format!("Thumbnail task failed: {}", e))??;

    let mut generated_blurhash = false;
    let mut blurhash = current_blurhash;
    if (blurhash.is_none() || force) && regenerated_thumbnail {
        if let Some(thumb_bytes) = thumb_for_blurhash {
            if let Ok(bh) = crate::media_processing::blurhash::get_blurhash_from_thumbnail_bytes(&thumb_bytes)
            {
                db.set_blurhash(hash, Some(&bh)).await?;
                blurhash = Some(bh);
                generated_blurhash = true;
            }
        }
    }

    Ok(EnsureThumbnailResult {
        regenerated_thumbnail,
        generated_blurhash,
        has_thumbnail,
        blurhash,
    })
}

async fn backfill_missing_blurhashes_inner(
    db: &crate::sqlite::SqliteDatabase,
    blob_store: &std::sync::Arc<crate::blob_store::BlobStore>,
    limit: Option<usize>,
) -> Result<BackfillMissingBlurhashesResult, String> {
    let batch_limit = limit.unwrap_or(128).clamp(1, 1000);
    let hashes: Vec<String> = db
        .with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT hash FROM file
                 WHERE blurhash IS NULL
                 ORDER BY file_id ASC
                 LIMIT ?1",
            )?;
            let rows = stmt.query_map([batch_limit as i64], |row| row.get::<_, String>(0))?;
            rows.collect()
        })
        .await?;

    let mut regenerated_thumbnails = 0usize;
    let mut generated_blurhashes = 0usize;
    for hash in &hashes {
        if let Ok(result) = ensure_thumbnail_inner(db, blob_store, hash).await {
            if result.regenerated_thumbnail {
                regenerated_thumbnails += 1;
            }
            if result.generated_blurhash {
                generated_blurhashes += 1;
            }
        }
    }

    let remaining: i64 = db
        .with_read_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM file WHERE blurhash IS NULL",
                [],
                |row| row.get(0),
            )
        })
        .await?;

    Ok(BackfillMissingBlurhashesResult {
        processed: hashes.len(),
        regenerated_thumbnails,
        generated_blurhashes,
        remaining: remaining.max(0) as usize,
    })
}

async fn reanalyze_file_colors_inner(
    db: &crate::sqlite::SqliteDatabase,
    blob_store: &std::sync::Arc<crate::blob_store::BlobStore>,
    hash: &str,
) -> Result<ReanalyzeFileColorsResult, String> {
    let file = db
        .get_file_by_hash(hash)
        .await?
        .ok_or_else(|| format!("File not found in database: {}", hash))?;

    // Color analysis is only meaningful for images.
    if !file.mime.starts_with("image/") {
        db.set_file_colors(hash, Vec::new(), None).await?;
        db.emit_compiler_event(crate::sqlite::CompilerEvent::RebuildAll);
        return Ok(ReanalyzeFileColorsResult {
            colors_extracted: 0,
            dominant_color_hex: None,
        });
    }

    let ext = mime_to_extension(&file.mime).to_string();
    let h = hash.to_string();
    let bs = blob_store.clone();
    let colors =
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, f32, f32, f32)>, String> {
            let original = bs
                .find_original(&h, Some(&ext))
                .map_err(|e| format!("Blob error: {}", e))?
                .ok_or_else(|| format!("Original file not found for hash {}", h))?;

            let bytes = std::fs::read(&original.0)
                .map_err(|e| format!("Failed to read original file: {}", e))?;
            let img =
                image::load_from_memory(&bytes).map_err(|e| format!("Image decode failed: {}", e))?;
            let extracted = crate::media_processing::colors::extract_dominant_colors(&img, 8);
            Ok(extracted
                .iter()
                .map(|c| (c.hex.clone(), c.l as f32, c.a as f32, c.b as f32))
                .collect())
        })
        .await
        .map_err(|e| format!("Color extraction task failed: {}", e))??;

    let dominant_color_hex = colors.first().map(|(hex, _, _, _)| hex.clone());
    let colors_extracted = colors.len();

    db.set_file_colors(hash, colors, dominant_color_hex.clone())
        .await?;
    db.emit_compiler_event(crate::sqlite::CompilerEvent::RebuildAll);

    Ok(ReanalyzeFileColorsResult {
        colors_extracted,
        dominant_color_hex,
    })
}

async fn color_search(
    db: &crate::sqlite::SqliteDatabase,
    hex_color: String,
    max_distance: Option<f64>,
) -> Result<Vec<crate::types::ColorSearchResult>, String> {
    let max_dist = max_distance.unwrap_or(25.0).max(1.0).min(100.0);

    let hex = hex_color.trim_start_matches('#');
    if hex.len() != 6 {
        return Err(format!("Invalid hex color: {}", hex_color));
    }
    let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| "Invalid red component")?;
    let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| "Invalid green component")?;
    let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| "Invalid blue component")?;

    use palette::{IntoColor, Lab, Srgb};
    let srgb = Srgb::new(r, g, b);
    let lab: Lab = srgb.into_linear::<f32>().into_color();

    let target_l = lab.l as f64;
    let target_a = lab.a as f64;
    let target_b = lab.b as f64;

    let results = db
        .with_read_conn(move |conn| {
            let l_range = max_dist;
            let a_range = max_dist * 2.0;
            let b_range = max_dist * 2.0;
            let mut stmt = conn.prepare(
                "SELECT fc.l, fc.a, fc.b, f.hash
                 FROM file_color_rtree rt
                 JOIN file_color fc ON fc.rowid = rt.id
                 JOIN file f ON f.file_id = fc.file_id
                 WHERE rt.l_max >= ?1 AND rt.l_min <= ?2
                   AND rt.a_max >= ?3 AND rt.a_min <= ?4
                   AND rt.b_max >= ?5 AND rt.b_min <= ?6",
            )?;
            let rows = stmt.query_map(
                rusqlite::params![
                    target_l - l_range,
                    target_l + l_range,
                    target_a - a_range,
                    target_a + a_range,
                    target_b - b_range,
                    target_b + b_range,
                ],
                |row| {
                    Ok((
                        row.get::<_, f64>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, f64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )?;

            let mut results = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for row in rows {
                let (l, a, b, hash) = row?;
                let dl = target_l - l;
                let da = target_a - a;
                let db_val = target_b - b;
                let distance = (dl * dl + da * da + db_val * db_val).sqrt();
                if distance <= max_dist && seen.insert(hash.clone()) {
                    results.push(crate::types::ColorSearchResult { hash, distance });
                }
            }
            results.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());
            Ok(results)
        })
        .await?;

    Ok(results)
}

// ─── TypedCommand impls ────────────────────────────────────────────────────

impl TypedCommand for ResolveFilePath {
    const NAME: &'static str = "resolve_file_path";
    type Input = ResolveFilePathInput;
    type Output = String;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        resolve_file_path_inner(&state.db, &state.blob_store, &input.hash).await
    }
}

impl TypedCommand for OpenFileDefault {
    const NAME: &'static str = "open_file_default";
    type Input = OpenFileDefaultInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let path = resolve_file_path_inner(&state.db, &state.blob_store, &input.hash).await?;
        open::that(&path).map_err(|e| format!("Failed to open file: {}", e))?;
        Ok(())
    }
}

impl TypedCommand for RevealInFolder {
    const NAME: &'static str = "reveal_in_folder";
    type Input = RevealInFolderInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let path = resolve_file_path_inner(&state.db, &state.blob_store, &input.hash).await?;
        reveal_in_folder_inner(&path)?;
        Ok(())
    }
}

impl TypedCommand for ExportFile {
    const NAME: &'static str = "export_file";
    type Input = ExportFileInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        export_file_inner(&state.db, &state.blob_store, &input.hash, &input.dest_path).await
    }
}

impl TypedCommand for OpenInNewWindow {
    const NAME: &'static str = "open_in_new_window";
    type Input = OpenInNewWindowInput;
    type Output = ();

    async fn execute(_state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::events::emit(
            crate::events::event_names::OPEN_DETAIL_WINDOW,
            &crate::events::OpenDetailWindowEvent {
                hash: input.hash,
                width: input.width,
                height: input.height,
            },
        );
        Ok(())
    }
}

impl TypedCommand for ResolveThumbnailPath {
    const NAME: &'static str = "resolve_thumbnail_path";
    type Input = ResolveThumbnailPathInput;
    type Output = String;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let bs = state.blob_store.clone();
        let hash = input.hash;
        let result = tokio::task::spawn_blocking(move || {
            bs.find_thumbnail_path(&hash)
                .map_err(|e| format!("Blob error: {}", e))?
                .map(|p| p.to_string_lossy().into_owned())
                .ok_or_else(|| format!("Thumbnail not found: {}", hash))
        })
        .await
        .map_err(|e| format!("Task error: {}", e))?;
        result
    }
}

impl TypedCommand for EnsureThumbnail {
    const NAME: &'static str = "ensure_thumbnail";
    type Input = EnsureThumbnailInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = ensure_thumbnail_inner(&state.db, &state.blob_store, &input.hash).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for RegenerateThumbnail {
    const NAME: &'static str = "regenerate_thumbnail";
    type Input = RegenerateThumbnailInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result =
            generate_thumbnail_inner(&state.db, &state.blob_store, &input.hash, true).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for RegenerateThumbnailsBatch {
    const NAME: &'static str = "regenerate_thumbnails_batch";
    type Input = RegenerateThumbnailsBatchInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let mut regenerated = 0usize;
        let mut errors = 0usize;
        for hash in &input.hashes {
            match generate_thumbnail_inner(&state.db, &state.blob_store, hash, true).await {
                Ok(r) => {
                    if r.regenerated_thumbnail {
                        regenerated += 1;
                    }
                }
                Err(_) => {
                    errors += 1;
                }
            }
        }
        Ok(serde_json::json!({
            "total": input.hashes.len(),
            "regenerated": regenerated,
            "errors": errors,
        }))
    }
}

impl TypedCommand for ReanalyzeFileColors {
    const NAME: &'static str = "reanalyze_file_colors";
    type Input = ReanalyzeFileColorsInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result =
            reanalyze_file_colors_inner(&state.db, &state.blob_store, &input.hash).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for BackfillMissingBlurhashes {
    const NAME: &'static str = "backfill_missing_blurhashes";
    type Input = BackfillMissingBlurhashesInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result =
            backfill_missing_blurhashes_inner(&state.db, &state.blob_store, input.limit).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for SearchByColor {
    const NAME: &'static str = "search_by_color";
    type Input = SearchByColorInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = color_search(&state.db, input.hex_color, input.max_distance).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for GetImageThumbnail {
    const NAME: &'static str = "get_image_thumbnail";
    type Input = GetImageThumbnailInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let bs = state.blob_store.clone();
        let hash = input.hash;
        let result = tokio::task::spawn_blocking(move || {
            bs.read_thumbnail(&hash).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("Task error: {}", e))?;
        let data = result?;
        serde_json::to_value(&data).map_err(|e| e.to_string())
    }
}

// ─── Dispatch router ───────────────────────────────────────────────────────

pub async fn dispatch_typed(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        ResolveFilePath::NAME => Some(run_typed::<ResolveFilePath>(state, args).await),
        OpenFileDefault::NAME => Some(run_typed::<OpenFileDefault>(state, args).await),
        RevealInFolder::NAME => Some(run_typed::<RevealInFolder>(state, args).await),
        ExportFile::NAME => Some(run_typed::<ExportFile>(state, args).await),
        OpenInNewWindow::NAME => Some(run_typed::<OpenInNewWindow>(state, args).await),
        ResolveThumbnailPath::NAME => Some(run_typed::<ResolveThumbnailPath>(state, args).await),
        EnsureThumbnail::NAME => Some(run_typed::<EnsureThumbnail>(state, args).await),
        RegenerateThumbnail::NAME => Some(run_typed::<RegenerateThumbnail>(state, args).await),
        RegenerateThumbnailsBatch::NAME => {
            Some(run_typed::<RegenerateThumbnailsBatch>(state, args).await)
        }
        ReanalyzeFileColors::NAME => Some(run_typed::<ReanalyzeFileColors>(state, args).await),
        BackfillMissingBlurhashes::NAME => {
            Some(run_typed::<BackfillMissingBlurhashes>(state, args).await)
        }
        SearchByColor::NAME => Some(run_typed::<SearchByColor>(state, args).await),
        GetImageThumbnail::NAME => Some(run_typed::<GetImageThumbnail>(state, args).await),
        _ => None,
    }
}
