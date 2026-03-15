//! Gallery-dl subprocess runner.
//!
//! Manages gallery-dl invocations: generates temp config files, spawns the
//! subprocess, scans output directories, and parses metadata sidecar JSON files.
//!
//! Gallery-dl reference: `external/gallery-dl/` (source code).
//! Key source files consulted:
//! - `gallery_dl/option.py` — CLI flag definitions (argparse)
//! - `gallery_dl/job.py` — DownloadJob, skip/abort logic (lines 621-632)
//! - `gallery_dl/postprocessor/metadata.py` — sidecar JSON writer
//! - `gallery_dl/archive.py` — SQLite download archive
//! - `gallery_dl/extractor/danbooru.py` — tag_string_* fields
//! - `gallery_dl/extractor/e621.py` — nested tags dict

mod config;
mod failure;
mod metadata_validation;
mod sites;

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::credential_store::SiteCredential;

use self::config::build_config;

pub use failure::{classify_failure, FailureKind};
pub use metadata_validation::{
    get_site_metadata_schema, validate_site_metadata, SiteMetadataSchema,
    SiteMetadataValidationResult,
};
pub use sites::{
    build_url, canonical_site_id, extract_domain, site_by_id, substitute_query, SiteEntry, SITES,
};

pub struct RunOptions {
    /// Full URL to download from (after query substitution).
    pub url: String,
    /// Max files to download (maps to `--range 1-N`). None = unlimited.
    pub file_limit: Option<u32>,
    /// Abort after N consecutive skipped files (maps to `-A N`).
    /// None = no abort (first run / initial sync).
    pub abort_threshold: Option<u32>,
    /// Seconds between HTTP requests during extraction (`sleep-request`).
    pub sleep_request: f64,
    /// Optional credential for site authentication.
    pub credential: Option<SiteCredential>,
    /// Path to the download archive SQLite DB.
    pub archive_path: PathBuf,
    /// Optional archive key prefix (used to support targeted reset per subscription/query).
    pub archive_prefix: Option<String>,
    /// Cancellation token — kills the subprocess when cancelled.
    pub cancel: CancellationToken,
}

/// Result of a gallery-dl invocation.
pub struct RunResult {
    pub items: Vec<DownloadedItem>,
    pub exit_code: i32,
    pub stderr_output: String,
}

/// A single file downloaded by gallery-dl, paired with its parsed metadata.
pub struct DownloadedItem {
    pub file_path: PathBuf,
    pub metadata: ParsedMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsedMetadata {
    /// Tags as (namespace, subtag) pairs.
    pub tags: Vec<(String, String)>,
    pub description: Option<String>,
    pub source_url: Option<String>,
    pub rating: Option<String>,
    pub title: Option<String>,
    pub post_id: Option<String>,
    /// Gallery-dl extractor category (e.g. "danbooru", "pixiv").
    pub category: Option<String>,
}

/// The gallery-dl subprocess runner.
pub struct GalleryDlRunner {
    binary_path: PathBuf,
}

impl GalleryDlRunner {
    pub fn new(binary_path: PathBuf) -> Self {
        Self { binary_path }
    }

    /// Run gallery-dl and return downloaded items with parsed metadata.
    pub async fn run(&self, opts: &RunOptions) -> Result<RunResult, String> {
        self.ensure_runtime_dependencies().await?;

        // 1. Create temp download directory
        let temp_dir =
            std::env::temp_dir().join(format!("picto_gdl_{:016x}", rand::random::<u64>()));
        tokio::fs::create_dir_all(&temp_dir)
            .await
            .map_err(|e| format!("Failed to create temp dir: {e}"))?;

        // 2. Build and write temp config
        let config = build_config(opts, &temp_dir);
        let config_path = temp_dir.join("config.json");
        let config_json = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Config serialization error: {e}"))?;
        tokio::fs::write(&config_path, &config_json)
            .await
            .map_err(|e| format!("Config write error: {e}"))?;

        // 3. Build command arguments
        let mut args = vec![
            "--config".to_string(),
            config_path.display().to_string(),
            "--config-ignore".to_string(), // don't read user's default configs
            "--write-metadata".to_string(),
            "--no-input".to_string(),
            "-d".to_string(),
            temp_dir.display().to_string(),
        ];

        if let Some(limit) = opts.file_limit {
            args.push("--range".to_string());
            args.push(format!("1-{limit}"));
        }

        if let Some(threshold) = opts.abort_threshold {
            args.push("-A".to_string());
            args.push(threshold.to_string());
        }

        if !opts.archive_path.as_os_str().is_empty() {
            args.push("--download-archive".to_string());
            args.push(opts.archive_path.display().to_string());
        }

        args.push(opts.url.clone());

        info!(
            url = %opts.url,
            file_limit = ?opts.file_limit,
            abort_threshold = ?opts.abort_threshold,
            "Spawning gallery-dl"
        );
        debug!(binary = %self.binary_path.display(), args = ?args, "gallery-dl command");

        // 4. Spawn subprocess
        let mut child = tokio::process::Command::new(&self.binary_path)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to spawn gallery-dl: {e}"))?;

        // 5. Capture stderr handle, then wait for exit or cancellation
        let child_stderr = child.stderr.take();
        let child_stdout = child.stdout.take();

        let status = tokio::select! {
            _ = opts.cancel.cancelled() => {
                info!("Gallery-dl cancelled, killing subprocess");
                let _ = child.kill().await;
                child.wait().await
                    .map_err(|e| format!("Failed to wait for gallery-dl after kill: {e}"))?
            }
            result = child.wait() => {
                result.map_err(|e| format!("Gallery-dl process error: {e}"))?
            }
        };

        let exit_code = status.code().unwrap_or(-1);

        // Read stderr for logging
        let stderr = if let Some(mut se) = child_stderr {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            let _ = se.read_to_end(&mut buf).await;
            String::from_utf8_lossy(&buf).to_string()
        } else {
            String::new()
        };
        drop(child_stdout);

        if !stderr.is_empty() {
            for line in stderr.lines().take(20) {
                debug!(line, "gallery-dl stderr");
            }
        }

        info!(exit_code, "gallery-dl finished");

        // 6. Scan output directory for downloaded files + metadata sidecars
        let items = scan_output_dir(&temp_dir).await?;

        // 7. Clean up temp config (leave downloaded files for caller to import)
        let _ = tokio::fs::remove_file(&config_path).await;

        Ok(RunResult {
            items,
            exit_code,
            stderr_output: stderr,
        })
    }

    async fn ensure_runtime_dependencies(&self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            let vendor_marker = format!(
                "{}vendor{}gallery-dl{}gallery-dl",
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR
            );
            let bin = self.binary_path.to_string_lossy();
            if !bin.contains(&vendor_marker) {
                return Ok(());
            }

            let vendor_dir = self
                .binary_path
                .parent()
                .ok_or_else(|| "Invalid gallery-dl vendor path".to_string())?;
            let wheel_dir = vendor_dir.join("wheel");
            let existing_py = std::env::var("PYTHONPATH").unwrap_or_default();
            let merged_py = if existing_py.is_empty() {
                wheel_dir.display().to_string()
            } else {
                format!("{}:{}", wheel_dir.display(), existing_py)
            };

            let check_status = tokio::process::Command::new("python3")
                .arg("-c")
                .arg("import requests")
                .env("PYTHONPATH", &merged_py)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .map_err(|e| format!("Failed to validate gallery-dl python deps: {e}"))?;
            if check_status.success() {
                return Ok(());
            }

            warn!("gallery-dl dependency bootstrap: installing missing Python package 'requests'");
            let install_status = tokio::process::Command::new("python3")
                .args([
                    "-m",
                    "pip",
                    "install",
                    "--disable-pip-version-check",
                    "--quiet",
                    "--target",
                ])
                .arg(&wheel_dir)
                .arg("requests")
                .status()
                .await
                .map_err(|e| format!("Failed to run pip for gallery-dl dependencies: {e}"))?;
            if !install_status.success() {
                return Err(
                    "gallery-dl is missing Python dependency 'requests' and auto-install failed. Run `bash scripts/download-gallery-dl.sh`."
                        .to_string(),
                );
            }

            let recheck_status = tokio::process::Command::new("python3")
                .arg("-c")
                .arg("import requests")
                .env("PYTHONPATH", &merged_py)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .map_err(|e| format!("Failed to re-validate gallery-dl python deps: {e}"))?;
            if !recheck_status.success() {
                return Err(
                    "gallery-dl dependency check still failing after install. Run `bash scripts/download-gallery-dl.sh`."
                        .to_string(),
                );
            }
        }

        Ok(())
    }
}

/// After import, call this to remove the temp download directory.
pub async fn cleanup_temp_dir(temp_dir: &Path) {
    if let Err(e) = tokio::fs::remove_dir_all(temp_dir).await {
        warn!(path = %temp_dir.display(), error = %e, "Failed to clean up temp dir");
    }
}

/// Walk the temp directory and pair each media file with its `.json` sidecar.
async fn scan_output_dir(dir: &Path) -> Result<Vec<DownloadedItem>, String> {
    let mut items = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&current)
            .await
            .map_err(|e| format!("Read dir error: {e}"))?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            // Skip JSON sidecars and config files — we'll find them when processing media files
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "json" {
                continue;
            }

            // Look for matching sidecar: {filename}.json
            let sidecar_path = path.with_extension(format!(
                "{}.json",
                path.extension().and_then(|e| e.to_str()).unwrap_or("")
            ));

            let metadata = if sidecar_path.is_file() {
                match tokio::fs::read_to_string(&sidecar_path).await {
                    Ok(json_str) => match serde_json::from_str::<serde_json::Value>(&json_str) {
                        Ok(json) => parse_metadata(&json),
                        Err(e) => {
                            warn!(path = %sidecar_path.display(), error = %e, "Sidecar parse error");
                            ParsedMetadata::default()
                        }
                    },
                    Err(e) => {
                        warn!(path = %sidecar_path.display(), error = %e, "Sidecar read error");
                        ParsedMetadata::default()
                    }
                }
            } else {
                debug!(path = %path.display(), "No sidecar found");
                ParsedMetadata::default()
            };

            items.push(DownloadedItem {
                file_path: path,
                metadata,
            });
        }
    }

    Ok(items)
}

/// Parse a gallery-dl metadata sidecar JSON into normalized metadata.
///
/// Handles site-specific tag formats:
/// - Danbooru: `tags_artist`, `tags_general`, etc. (arrays from space-split `tag_string_*`)
/// - E621: `tags` dict with category arrays (`{"general": [...], "artist": [...]}`)
/// - Pixiv: `tags` array of objects (`[{"name": "...", "translated_name": "..."}]`)
/// - Fallback: `tags` as flat array of strings or space-separated string
pub fn parse_metadata(json: &serde_json::Value) -> ParsedMetadata {
    let mut tags = parse_tags(json);
    if let Some(creator) = extract_creator_identifier(json) {
        if !tags
            .iter()
            .any(|(ns, subtag)| ns == "creator" && subtag == &creator)
        {
            tags.push(("creator".to_string(), creator));
        }
    }

    // Try artist_commentary (Danbooru with metadata: true), then direct fields.
    let description = json
        .get("artist_commentary")
        .and_then(|ac| {
            ac.get("original_description")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            json.get("description")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            json.get("caption")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            json.get("body")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            json.get("content")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .map(String::from);

    let source_url = json
        .get("file_url")
        .or_else(|| json.get("url"))
        .or_else(|| json.get("source"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    let rating = json.get("rating").and_then(|v| {
        v.as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| v.as_i64().map(|value| value.to_string()))
    });

    let title = json
        .get("artist_commentary")
        .and_then(|ac| {
            ac.get("original_title")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            json.get("title")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .map(String::from);

    let post_id = json
        .get("id")
        .map(|v| {
            if let Some(n) = v.as_i64() {
                n.to_string()
            } else {
                v.as_str().unwrap_or("").to_string()
            }
        })
        .filter(|s| !s.is_empty());

    let category = json
        .get("category")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    ParsedMetadata {
        tags,
        description,
        source_url,
        rating,
        title,
        post_id,
        category,
    }
}

/// Parse tags from a gallery-dl metadata sidecar.
///
/// Priority order:
/// 1. Danbooru-style: `tags_artist`, `tags_character`, `tags_copyright`,
///    `tags_general`, `tags_meta` (arrays)
/// 2. E621/nested: `tags` as object with category keys → arrays
/// 3. Pixiv: `tags` as array of `{"name": "...", "translated_name": "..."}` objects
/// 4. Fallback: `tags` as flat array of strings or space-separated string
pub fn parse_tags(json: &serde_json::Value) -> Vec<(String, String)> {
    let mut tags = Vec::new();

    // 1. Danbooru-style: tags_artist, tags_general, etc.
    static DANBOORU_CATEGORIES: &[(&str, &str)] = &[
        ("tags_artist", "creator"),
        ("tags_character", "character"),
        ("tags_copyright", "series"),
        ("tags_general", ""),
        ("tags_meta", "meta"),
    ];

    let has_danbooru = DANBOORU_CATEGORIES
        .iter()
        .any(|(key, _)| json.get(*key).is_some());

    if has_danbooru {
        for (key, namespace) in DANBOORU_CATEGORIES {
            if let Some(arr) = json.get(*key).and_then(|v| v.as_array()) {
                for tag_val in arr {
                    if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                        tags.push((namespace.to_string(), tag.to_string()));
                    }
                }
            }
        }
        if !tags.is_empty() {
            return tags;
        }
    }

    // 2. Danbooru/Gelbooru legacy strings: tag_string_*.
    static DANBOORU_TAG_STRINGS: &[(&str, &str)] = &[
        ("tag_string_artist", "creator"),
        ("tag_string_character", "character"),
        ("tag_string_copyright", "series"),
        ("tag_string_general", ""),
        ("tag_string_meta", "meta"),
    ];
    let has_tag_strings = DANBOORU_TAG_STRINGS
        .iter()
        .any(|(key, _)| json.get(*key).is_some());
    if has_tag_strings {
        for (key, namespace) in DANBOORU_TAG_STRINGS {
            if let Some(tag_string) = json.get(*key).and_then(|v| v.as_str()) {
                for tag in tag_string.split_whitespace() {
                    if !tag.is_empty() {
                        tags.push((namespace.to_string(), tag.to_string()));
                    }
                }
            }
        }
        if !tags.is_empty() {
            return tags;
        }
    }

    // 3. Try `tags` field.
    if let Some(tags_val) = json.get("tags") {
        // 3a. E621-style: tags is an object with category arrays
        if let Some(obj) = tags_val.as_object() {
            // Check if values are arrays (E621) vs other structure
            let is_category_dict = obj.values().any(|v| v.is_array());
            if is_category_dict {
                static E621_NAMESPACE_MAP: &[(&str, &str)] = &[
                    ("artist", "creator"),
                    ("character", "character"),
                    ("copyright", "series"),
                    ("general", ""),
                    ("meta", "meta"),
                    ("species", "species"),
                    ("lore", "lore"),
                ];
                for (category, namespace) in E621_NAMESPACE_MAP {
                    if let Some(arr) = obj.get(*category).and_then(|v| v.as_array()) {
                        for tag_val in arr {
                            if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                                tags.push((namespace.to_string(), tag.to_string()));
                            }
                        }
                    }
                }
                // Also collect any categories not in our map
                for (category, value) in obj {
                    let mapped = E621_NAMESPACE_MAP
                        .iter()
                        .any(|(cat, _)| *cat == category.as_str());
                    if !mapped {
                        if let Some(arr) = value.as_array() {
                            for tag_val in arr {
                                if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                                    tags.push((category.clone(), tag.to_string()));
                                }
                            }
                        }
                    }
                }
                return tags;
            }
        }

        // 3b. Pixiv-style: tags is array of objects with "name" field
        if let Some(arr) = tags_val.as_array() {
            if arr.first().and_then(|v| v.as_object()).is_some() {
                // Array of tag objects
                for tag_obj in arr {
                    if let Some(name) = tag_obj.get("name").and_then(|v| v.as_str()) {
                        if !name.is_empty() {
                            tags.push((String::new(), name.to_string()));
                        }
                    }
                }
                return tags;
            }

            // 3c. Flat array of strings
            for tag_val in arr {
                if let Some(tag) = tag_val.as_str().filter(|s| !s.is_empty()) {
                    tags.push((String::new(), tag.to_string()));
                }
            }
            return tags;
        }

        // 3d. Space-separated string (rare but possible)
        if let Some(tag_str) = tags_val.as_str() {
            for tag in tag_str.split_whitespace() {
                if !tag.is_empty() {
                    tags.push((String::new(), tag.to_string()));
                }
            }
            if !tags.is_empty() {
                return tags;
            }
        }
    }

    // 4. Gelbooru fallback: plain tag_string with no namespace metadata.
    if let Some(tag_str) = json.get("tag_string").and_then(|v| v.as_str()) {
        for tag in tag_str.split_whitespace() {
            if !tag.is_empty() {
                tags.push((String::new(), tag.to_string()));
            }
        }
    }

    tags
}

pub fn extract_creator_identifier(json: &serde_json::Value) -> Option<String> {
    let user = json.get("user")?;
    if let Some(name) = user
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(name.to_string());
    }
    if let Some(id) = user.get("id") {
        if let Some(n) = id.as_i64() {
            return Some(n.to_string());
        }
        if let Some(s) = id.as_str().map(str::trim).filter(|v| !v.is_empty()) {
            return Some(s.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_danbooru_tags() {
        let json = serde_json::json!({
            "id": 12345,
            "tags_artist": ["artist_name"],
            "tags_character": ["char_a", "char_b"],
            "tags_copyright": ["series_name"],
            "tags_general": ["1girl", "solo", "blue_eyes"],
            "tags_meta": ["highres"]
        });
        let tags = parse_tags(&json);
        assert_eq!(tags.len(), 8); // 1 artist + 2 char + 1 copyright + 3 general + 1 meta
        assert!(tags.contains(&("creator".to_string(), "artist_name".to_string())));
        assert!(tags.contains(&("character".to_string(), "char_a".to_string())));
        assert!(tags.contains(&("character".to_string(), "char_b".to_string())));
        assert!(tags.contains(&("series".to_string(), "series_name".to_string())));
        assert!(tags.contains(&(String::new(), "1girl".to_string())));
        assert!(tags.contains(&("meta".to_string(), "highres".to_string())));
    }

    #[test]
    fn test_parse_e621_tags() {
        let json = serde_json::json!({
            "id": 67890,
            "tags": {
                "general": ["anthro", "solo"],
                "artist": ["artist_x"],
                "character": ["char_y"],
                "copyright": ["series_z"],
                "species": ["canine"],
                "meta": ["hi_res"]
            }
        });
        let tags = parse_tags(&json);
        assert_eq!(tags.len(), 7); // 2 + 1 + 1 + 1 + 1 + 1
        assert!(tags.contains(&("creator".to_string(), "artist_x".to_string())));
        assert!(tags.contains(&("species".to_string(), "canine".to_string())));
        assert!(tags.contains(&(String::new(), "anthro".to_string())));
    }

    #[test]
    fn test_parse_pixiv_tags() {
        let json = serde_json::json!({
            "id": 99999,
            "tags": [
                {"name": "オリジナル", "translated_name": "original"},
                {"name": "女の子", "translated_name": "girl"},
                {"name": "風景", "translated_name": null}
            ]
        });
        let tags = parse_tags(&json);
        assert_eq!(tags.len(), 3);
        assert!(tags.contains(&(String::new(), "オリジナル".to_string())));
        assert!(tags.contains(&(String::new(), "女の子".to_string())));
        assert!(tags.contains(&(String::new(), "風景".to_string())));
    }

    #[test]
    fn test_parse_metadata_artist_commentary() {
        // Danbooru with metadata: true provides artist_commentary object
        let json = serde_json::json!({
            "id": 10873290,
            "tag_string_artist": "h4sh1rnoto",
            "tag_string_general": "1girl blonde_hair",
            "tag_string_character": "princess_peach",
            "tag_string_copyright": "mario_(series)",
            "tag_string_meta": "highres",
            "artist_commentary": {
                "original_title": "ピーチ姫",
                "original_description": "マリオシリーズ\r\n#イラスト #illustration",
                "translated_title": "",
                "translated_description": ""
            },
            "file_url": "https://cdn.donmai.us/original/test.jpg",
            "category": "danbooru"
        });
        let meta = parse_metadata(&json);
        assert_eq!(meta.title.as_deref(), Some("ピーチ姫"));
        assert_eq!(
            meta.description.as_deref(),
            Some("マリオシリーズ\r\n#イラスト #illustration")
        );
        assert_eq!(meta.post_id.as_deref(), Some("10873290"));
    }

    #[test]
    fn test_parse_metadata_artist_commentary_empty_falls_back() {
        // When artist_commentary fields are empty, fall back to direct fields
        let json = serde_json::json!({
            "id": 1,
            "artist_commentary": {
                "original_title": "",
                "original_description": ""
            },
            "description": "A direct description",
            "title": "Direct title",
            "category": "danbooru"
        });
        let meta = parse_metadata(&json);
        assert_eq!(meta.title.as_deref(), Some("Direct title"));
        assert_eq!(meta.description.as_deref(), Some("A direct description"));
    }

    #[test]
    fn test_substitute_query() {
        assert_eq!(
            substitute_query(
                "https://danbooru.donmai.us/posts?tags={query}",
                "1girl solo"
            ),
            "https://danbooru.donmai.us/posts?tags=1girl+solo"
        );
        assert_eq!(
            substitute_query(
                "https://e621.net/posts?tags={query}",
                "rating:safe order:score"
            ),
            "https://e621.net/posts?tags=rating%3Asafe+order%3Ascore"
        );
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            extract_domain("https://danbooru.donmai.us/posts?tags=1girl"),
            Some("danbooru.donmai.us".to_string())
        );
        assert_eq!(
            extract_domain("https://www.pixiv.net/artworks/12345"),
            Some("www.pixiv.net".to_string())
        );
        assert_eq!(extract_domain("not-a-url"), None);
    }

    #[test]
    fn test_site_by_id() {
        let dan = site_by_id("danbooru").unwrap();
        assert_eq!(dan.name, "Danbooru");
        assert!(dan.url_template.contains("{query}"));

        assert!(site_by_id("nonexistent_site_xyz").is_none());
        assert_eq!(site_by_id("rule34xxx").unwrap().id, "rule34");
        assert_eq!(canonical_site_id("rule34xxx"), "rule34");
    }

    #[test]
    fn test_build_url() {
        assert_eq!(
            build_url("danbooru", "1girl solo").unwrap(),
            "https://danbooru.donmai.us/posts?tags=1girl+solo"
        );
        assert_eq!(
            build_url("e621", "canine rating:safe").unwrap(),
            "https://e621.net/posts?tags=canine+rating%3Asafe"
        );
        assert_eq!(
            build_url("pixiv", "風景").unwrap(),
            "https://www.pixiv.net/en/tags/%E9%A2%A8%E6%99%AF/artworks?s_mode=s_tag"
        );
        assert!(build_url("nonexistent", "query").is_none());
    }

    #[test]
    fn test_classify_failure_unauthorized() {
        let kind = classify_failure("HTTP Error 403 Forbidden: Login required");
        assert_eq!(kind, FailureKind::Unauthorized);
    }

    #[test]
    fn test_site_capability_contract_representative_matrix() {
        let pixiv = site_by_id("pixiv").expect("pixiv");
        assert!(pixiv.supports_query);
        assert!(pixiv.supports_account);
        assert!(pixiv.auth_supported);
        assert!(pixiv.auth_required_for_full_access);

        let tumblr = site_by_id("tumblr").expect("tumblr");
        assert!(tumblr.supports_query);
        assert!(tumblr.supports_account);
        assert!(!tumblr.auth_supported);
        assert!(!tumblr.auth_required_for_full_access);

        let patreon = site_by_id("patreon").expect("patreon");
        assert!(!patreon.supports_query);
        assert!(patreon.supports_account);
        assert!(patreon.auth_supported);
    }

    #[test]
    fn test_site_contract_auth_required_implies_auth_supported() {
        for site in SITES {
            assert!(
                !site.auth_required_for_full_access || site.auth_supported,
                "site {} requires auth for full access but is marked auth unsupported",
                site.id
            );
        }
    }

    #[test]
    fn test_build_url_contract_for_query_and_account_templates() {
        assert_eq!(
            build_url("patreon", "creatorname").as_deref(),
            Some("https://www.patreon.com/creatorname/posts")
        );
        assert_eq!(
            build_url("tumblr", "myblog").as_deref(),
            Some("https://myblog.tumblr.com")
        );
        assert_eq!(
            build_url("rule34xxx", "solo").as_deref(),
            Some("https://rule34.xxx/index.php?page=post&s=list&tags=solo")
        );
    }

    #[test]
    fn test_parse_metadata_extracts_pixiv_creator_tag() {
        let json = serde_json::json!({
            "id": 100,
            "title": "Pixiv work",
            "url": "https://www.pixiv.net/artworks/100",
            "tags": [{"name":"landscape","translated_name":null}],
            "user": {"id": 77, "name": "artist_name"},
            "page_count": 1,
            "category": "pixiv"
        });
        let meta = parse_metadata(&json);
        assert!(meta
            .tags
            .iter()
            .any(|(ns, subtag)| ns == "creator" && subtag == "artist_name"));
    }

    #[test]
    fn test_validate_site_metadata_pixiv_valid_payload() {
        let json = serde_json::json!({
            "id": 123,
            "title": "Pixiv title",
            "caption": "Pixiv caption",
            "url": "https://www.pixiv.net/artworks/123",
            "tags": [{"name":"tag_a","translated_name":null}],
            "user": {"id": 55, "name": "pixiv_user"},
            "page_count": 3,
            "category": "pixiv"
        });
        let res =
            validate_site_metadata("pixiv", "https://www.pixiv.net/artworks/123", Some(&json));
        assert!(res.valid, "validation errors: {:?}", res.invalid_fields);
        assert!(res.missing_required_fields.is_empty());
        assert!(res.invalid_fields.is_empty());
        assert!(res.normalized_preview.is_some());
    }

    #[test]
    fn test_validate_site_metadata_pixiv_missing_required_keys() {
        let json = serde_json::json!({
            "id": 123,
            "tags": [],
            "user": {},
            "category": "pixiv"
        });
        let res = validate_site_metadata("pixiv", "", Some(&json));
        assert!(!res.valid);
        assert!(res
            .missing_required_fields
            .contains(&"title|caption".to_string()));
        assert!(res
            .missing_required_fields
            .contains(&"page_count|meta_pages".to_string()));
        assert!(res
            .missing_required_fields
            .contains(&"url|file_url".to_string()));
    }

    #[test]
    fn test_validate_site_metadata_gelbooru_valid_payload() {
        let json = serde_json::json!({
            "id": 42,
            "tag_string": "1girl smile",
            "file_url": "https://img3.gelbooru.com/images/a/b/example.jpg",
            "source": "https://twitter.com/example/status/1",
            "rating": "safe",
            "md5": "0123456789abcdef0123456789abcdef",
            "category": "gelbooru"
        });
        let res = validate_site_metadata(
            "gelbooru",
            "https://gelbooru.com/index.php?page=post&s=view&id=42",
            Some(&json),
        );
        assert!(res.valid, "validation errors: {:?}", res.invalid_fields);
        assert!(res.missing_required_fields.is_empty());
        assert!(res.invalid_fields.is_empty());
        assert!(res.normalized_preview.is_some());
    }

    #[test]
    fn test_get_site_metadata_schema_gelbooru() {
        let schema = get_site_metadata_schema("gelbooru").expect("gelbooru schema");
        assert_eq!(schema.site_id, "gelbooru");
        assert!(
            schema
                .required_raw_keys
                .iter()
                .any(|k| k == "tags|tag_string"),
            "schema should accept tags or tag_string"
        );
    }

    #[test]
    fn test_validate_site_metadata_gelbooru_missing_required_keys() {
        let json = serde_json::json!({
            "id": 42,
            "tag_string": "",
            "rating": "safe",
            "category": "gelbooru"
        });
        let res = validate_site_metadata("gelbooru", "", Some(&json));
        assert!(!res.valid);
        assert!(res
            .missing_required_fields
            .contains(&"file_url".to_string()));
        assert!(res.missing_required_fields.contains(&"source".to_string()));
        assert!(res.invalid_fields.contains(&"tags[]".to_string()));
    }

    #[test]
    fn test_get_site_metadata_schema_danbooru() {
        let schema = get_site_metadata_schema("danbooru").expect("danbooru schema");
        assert_eq!(schema.site_id, "danbooru");
        assert!(
            schema
                .required_raw_keys
                .iter()
                .any(|k| k == "tags_artist|tags_general|category_tags"),
            "schema should require category tags"
        );
    }

    #[test]
    fn test_validate_site_metadata_danbooru_valid_payload() {
        let json = serde_json::json!({
            "id": 10873290,
            "tags_artist": ["h4sh1rnoto"],
            "tags_character": ["princess_peach"],
            "tags_copyright": ["mario_(series)"],
            "tags_general": ["1girl", "blonde_hair"],
            "tags_meta": ["highres"],
            "artist_commentary": {
                "original_title": "ピーチ姫",
                "original_description": "マリオシリーズ"
            },
            "file_url": "https://cdn.donmai.us/original/test.jpg",
            "source": "https://x.com/example/status/1",
            "rating": "s",
            "category": "danbooru"
        });
        let res = validate_site_metadata(
            "danbooru",
            "https://danbooru.donmai.us/posts/10873290",
            Some(&json),
        );
        assert!(res.valid, "validation errors: {:?}", res.invalid_fields);
        assert!(res.missing_required_fields.is_empty());
        assert!(res.invalid_fields.is_empty());
        assert!(res.normalized_preview.is_some());
    }

    #[test]
    fn test_validate_site_metadata_danbooru_missing_required_keys() {
        let json = serde_json::json!({
            "id": 10873290,
            "tags_general": ["1girl"],
            "file_url": "https://cdn.donmai.us/original/test.jpg",
            "category": "danbooru"
        });
        let res = validate_site_metadata("danbooru", "", Some(&json));
        assert!(!res.valid);
        assert!(res.missing_required_fields.contains(&"source".to_string()));
        assert!(res.missing_required_fields.contains(&"rating".to_string()));
        assert!(res.invalid_fields.contains(&"creator".to_string()));
    }
}
