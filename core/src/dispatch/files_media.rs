//! File media handlers: path resolution, OS integration (open, reveal, export),
//! thumbnails, blurhash backfill, and color search.

use crate::blob_store::mime_to_extension;
use crate::state::AppState;
use crate::types::*;

use super::common::{de, de_opt, ok_null, to_json};

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        "resolve_file_path" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = resolve_file_path_inner(&state.db, &state.blob_store, &hash).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "open_file_default" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let path = match resolve_file_path_inner(&state.db, &state.blob_store, &hash).await {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            Some(
                open::that(&path)
                    .map_err(|e| format!("Failed to open file: {}", e))
                    .and_then(|_| ok_null()),
            )
        }
        "reveal_in_folder" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let path = match resolve_file_path_inner(&state.db, &state.blob_store, &hash).await {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            Some(reveal_in_folder_inner(&path).and_then(|_| ok_null()))
        }
        "export_file" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let dest_path: String = match de(args, "dest_path") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = export_file_inner(&state.db, &state.blob_store, &hash, &dest_path).await;
            Some(result.and_then(|_| ok_null()))
        }
        "save_file_data" => {
            let file_path: String = match de(args, "filePath") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let data: Vec<u8> = match de(args, "data") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = tokio::task::spawn_blocking(move || {
                std::fs::write(&file_path, &data)
                    .map_err(|e| format!("Failed to write file: {}", e))
            })
            .await
            .map_err(|e| format!("Task error: {}", e));
            Some(match result {
                Ok(inner) => inner.and_then(|_| ok_null()),
                Err(e) => Err(e),
            })
        }
        "open_in_new_window" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let width: Option<u32> = de(args, "width").ok();
            let height: Option<u32> = de(args, "height").ok();
            crate::events::emit(
                crate::events::event_names::OPEN_DETAIL_WINDOW,
                &crate::events::OpenDetailWindowEvent {
                    hash,
                    width,
                    height,
                },
            );
            Some(ok_null())
        }

        "resolve_thumbnail_path" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let bs = state.blob_store.clone();
            let result = tokio::task::spawn_blocking(move || {
                bs.find_thumbnail_path(&hash)
                    .map_err(|e| format!("Blob error: {}", e))?
                    .map(|p| p.to_string_lossy().into_owned())
                    .ok_or_else(|| format!("Thumbnail not found: {}", hash))
            })
            .await
            .map_err(|e| format!("Task error: {}", e));
            Some(match result {
                Ok(inner) => inner.and_then(|r| to_json(&r)),
                Err(e) => Err(e),
            })
        }
        "ensure_thumbnail" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = ensure_thumbnail_inner(&state.db, &state.blob_store, &hash).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "regenerate_thumbnail" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = generate_thumbnail_inner(&state.db, &state.blob_store, &hash, true).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "regenerate_thumbnails_batch" => {
            let hashes: Vec<String> = match de(args, "hashes") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let mut regenerated = 0usize;
            let mut errors = 0usize;
            for hash in &hashes {
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
            Some(to_json(&serde_json::json!({
                "total": hashes.len(),
                "regenerated": regenerated,
                "errors": errors,
            })))
        }
        "backfill_missing_blurhashes" => {
            let limit: Option<usize> = de_opt(args, "limit");
            let result =
                backfill_missing_blurhashes_inner(&state.db, &state.blob_store, limit).await;
            Some(result.and_then(|r| to_json(&r)))
        }

        "search_by_color" => {
            let hex_color: String = match de(args, "hex_color") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let max_distance: Option<f64> = de_opt(args, "max_distance");
            let result = color_search(&state.db, hex_color, max_distance).await;
            Some(result.and_then(|r| to_json(&r)))
        }

        "get_image_thumbnail" => {
            let hash: String = match args
                .get("imageId")
                .or_else(|| args.get("hash"))
                .and_then(|v| serde_json::from_value::<String>(v.clone()).ok())
            {
                Some(h) => h,
                None => return Some(Err("Missing imageId or hash".to_string())),
            };
            let bs = state.blob_store.clone();
            let result = tokio::task::spawn_blocking(move || {
                bs.read_thumbnail(&hash).map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| format!("Task error: {}", e));
            Some(match result {
                Ok(inner) => inner.and_then(|r| to_json(&r)),
                Err(e) => Err(e),
            })
        }

        _ => None,
    }
}

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

            let info = crate::files::get_file_info(&original.0, None)
                .map_err(|e| format!("File info failed: {}", e))?;
            let (thumb_bytes, thumb_ext) = crate::files::generate_thumbnail_bytes(
                &original.0,
                crate::files::DEFAULT_THUMBNAIL_DIMENSIONS,
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
            if let Ok(bh) = crate::files::blurhash::get_blurhash_from_thumbnail_bytes(&thumb_bytes)
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

async fn color_search(
    db: &crate::sqlite::SqliteDatabase,
    hex_color: String,
    max_distance: Option<f64>,
) -> Result<Vec<ColorSearchResult>, String> {
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
                    results.push(ColorSearchResult { hash, distance });
                }
            }
            results.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());
            Ok(results)
        })
        .await?;

    Ok(results)
}
