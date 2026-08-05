//! Gallery-dl Python bridge runner.
//!
//! Manages gallery-dl invocations through the owned Python bridge:
//! generates temp config files, spawns the bridge, and consumes NDJSON item
//! events without relying on sidecar JSON as the live contract.
//!
//! Gallery-dl reference: `external/gallery-dl/` (source code).
//! Key source files consulted:
//! - `gallery_dl/option.py` — CLI flag definitions (argparse)
//! - `gallery_dl/job.py` — DownloadJob, skip/abort logic (lines 621-632)
//! - `gallery_dl/postprocessor/metadata.py` — sidecar JSON writer
//! - `gallery_dl/archive.py` — SQLite download archive
//! - `gallery_dl/extractor/danbooru.py` — tag_string_* fields
//! - `gallery_dl/extractor/e621.py` — nested tags dict

mod adapters;
mod config;
mod failure;
mod filesystem;
mod metadata;
mod metadata_validation;
mod sites;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Stdio;

use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::subscriptions::credential_service::GalleryDlAuthConfig;
use crate::subscriptions::source_adapter::{DownloadedItem, FailedDownloadedItem, ParsedMetadata};
use crate::tags::logging::summarize_tag_pairs;

use self::config::build_config;

pub use failure::{classify_failure, error_tail, final_error_line, FailureKind, RecoveryAction};
pub use filesystem::cleanup_temp_dir;
pub use metadata::{extract_creator_identifier, parse_metadata, parse_tags};
pub use metadata_validation::{
    get_site_metadata_schema, validate_site_metadata, SiteMetadataSchema,
    SiteMetadataValidationResult,
};
pub use sites::{
    build_url, canonical_site_id, credential_site_aliases, extract_domain, site_by_id,
    substitute_query, SiteEntry, SITES,
};

pub struct RunOptions {
    /// Optional subscription identifier for structured diagnostics.
    pub subscription_id: Option<i64>,
    /// Optional query identifier for structured diagnostics.
    pub query_id: Option<i64>,
    /// Site identifier used to derive the gallery-dl config.
    pub site_id: String,
    /// Full URL to download from (after query substitution).
    pub url: String,
    /// Max files to download (maps to `--post-range`). None = unlimited.
    pub post_limit: Option<u32>,
    /// Starting post index for `--post-range` (1-based). Used by range_offset pagination.
    pub range_start: u32,
    /// Abort after N consecutive skipped files (maps to `-A N`).
    /// None = no abort (first run / initial sync).
    pub abort_threshold: Option<u32>,
    /// Optional gallery-dl auth fragment for site authentication.
    pub auth: Option<GalleryDlAuthConfig>,
    /// Path to the download archive SQLite DB.
    pub archive_path: PathBuf,
    /// Optional archive key prefix (used to support targeted reset per subscription/query).
    pub archive_prefix: Option<String>,
    /// Cancellation token — kills the subprocess when cancelled.
    pub cancel: CancellationToken,
}

/// Summary of a gallery-dl invocation (no items — those are streamed via channel).
pub struct RunSummary {
    pub exit_code: i32,
    pub stderr_output: String,
    pub temp_dir: PathBuf,
    pub had_download_errors: bool,
    pub failed_items: Vec<FailedDownloadedItem>,
    pub discovered_items: usize,
    pub discovered_post_ids: Vec<String>,
    pub skipped_archive_items: usize,
}

#[derive(Debug, Deserialize)]
struct BridgeEvent {
    event: String,
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    metadata: Option<ParsedMetadata>,
}

#[derive(Default)]
struct BridgeOutputStats {
    failed_items: Vec<FailedDownloadedItem>,
    discovered_items: usize,
    discovered_post_ids: BTreeSet<String>,
    skipped_archive_items: usize,
}

fn config_bool_at<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<bool> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_bool()
}

fn metadata_has_raw_key(metadata: &ParsedMetadata, key: &str) -> bool {
    metadata
        .raw_metadata
        .as_ref()
        .and_then(|raw| raw.get(key))
        .is_some()
}

fn log_bridge_item_intake(metadata: &ParsedMetadata) {
    let summary = summarize_tag_pairs(&metadata.tags);
    debug!(
        post_id = metadata.post_id.as_deref().unwrap_or("?"),
        category = metadata.category.as_deref().unwrap_or("?"),
        item_key = metadata.item_key.as_deref().unwrap_or("?"),
        raw_has_tags_artist = metadata_has_raw_key(metadata, "tags_artist"),
        raw_has_tags_character = metadata_has_raw_key(metadata, "tags_character"),
        raw_has_tags_copyright = metadata_has_raw_key(metadata, "tags_copyright"),
        parsed_tag_count = summary.total,
        creator_tag_count = summary.creator,
        character_tag_count = summary.character,
        series_tag_count = summary.series,
        general_tag_count = summary.general,
        meta_tag_count = summary.meta,
        other_namespaced_tag_count = summary.other_namespaced,
        "gallery-dl bridge item intake"
    );
}

/// The gallery-dl subprocess runner.
pub struct GalleryDlRunner {
    binary_path: PathBuf,
}

impl GalleryDlRunner {
    pub fn new(binary_path: PathBuf) -> Self {
        Self { binary_path }
    }

    pub fn binary_path(&self) -> &PathBuf {
        &self.binary_path
    }

    /// Run gallery-dl, streaming downloaded items through `item_tx` as they arrive.
    /// Returns a summary (exit code, stderr) after the process finishes.
    pub async fn run(
        &self,
        opts: &RunOptions,
        item_tx: tokio::sync::mpsc::Sender<DownloadedItem>,
    ) -> Result<RunSummary, String> {
        let run_start = std::time::Instant::now();
        self.ensure_runtime_dependencies().await?;
        info!(
            elapsed_ms = run_start.elapsed().as_millis(),
            "gallery-dl: deps checked"
        );

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
        info!(
            subscription_id = opts.subscription_id,
            query_id = opts.query_id,
            site_id = %opts.site_id,
            gelbooru_tags_enabled = config_bool_at(&config, &["extractor", "gelbooru", "tags"]).unwrap_or(false),
            danbooru_tags_enabled = config_bool_at(&config, &["extractor", "danbooru", "tags"]).unwrap_or(false),
            request_interval_seconds = 1,
            metadata_enabled = config_bool_at(&config, &["extractor", "metadata"]).unwrap_or(false),
            config_path = %config_path.display(),
            "gallery-dl tag config prepared"
        );
        tokio::fs::write(&config_path, &config_json)
            .await
            .map_err(|e| format!("Config write error: {e}"))?;

        let bridge_request = serde_json::json!({
            "url": opts.url,
            "subscription_id": opts.subscription_id,
            "query_id": opts.query_id,
            "config_path": config_path.display().to_string(),
            "gallery_dl_module_dir": self.gallery_dl_module_dir().map(|path| path.display().to_string()),
            "post_range": opts.post_limit.map(|limit| {
                let start = opts.range_start.max(1);
                let end = start.saturating_add(limit).saturating_sub(1);
                format!("{start}-{end}")
            }),
            "abort_threshold": opts.abort_threshold,
            "archive_path": (!opts.archive_path.as_os_str().is_empty()).then(|| opts.archive_path.display().to_string()),
            "archive_prefix": opts.archive_prefix,
        });
        let request_path = temp_dir.join("bridge-request.json");
        tokio::fs::write(
            &request_path,
            serde_json::to_vec_pretty(&bridge_request)
                .map_err(|e| format!("Bridge request serialization error: {e}"))?,
        )
        .await
        .map_err(|e| format!("Bridge request write error: {e}"))?;

        let bridge_path = self.bridge_script_path()?;
        let python = self.python_executable();

        info!(
            url = %opts.url,
            post_limit = ?opts.post_limit,
            range_start = opts.range_start,
            abort_threshold = ?opts.abort_threshold,
            elapsed_ms = run_start.elapsed().as_millis(),
            "Spawning gallery-dl bridge"
        );
        info!(python = %python, bridge = %bridge_path.display(), "gallery-dl bridge command");

        // 4. Spawn subprocess
        let mut cmd = tokio::process::Command::new(&python);
        cmd.arg(&bridge_path)
            .arg("--request")
            .arg(&request_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(module_dir) = self.gallery_dl_module_dir() {
            let merged = match std::env::var("PYTHONPATH") {
                Ok(existing) if !existing.is_empty() => {
                    format!("{}:{}", module_dir.display(), existing)
                }
                _ => module_dir.display().to_string(),
            };
            cmd.env("PYTHONPATH", merged);
        }

        // On Windows: suppress console window and ensure clean process creation.
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn gallery-dl: {e}"))?;

        let child_stdout = child.stdout.take();
        let child_stderr = child.stderr.take();

        // 5. Stream stdout: bridge emits NDJSON events.
        use tokio::io::{AsyncBufReadExt, BufReader};
        let stdout_handle = tokio::spawn(async move {
            let mut stats = BridgeOutputStats::default();
            if let Some(out) = child_stdout {
                let mut reader = BufReader::new(out).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let Ok(event) = serde_json::from_str::<BridgeEvent>(trimmed) else {
                        tracing::warn!(line = trimmed, "gallery-dl bridge: invalid NDJSON event");
                        continue;
                    };
                    match event.event.as_str() {
                        "item_discovered" => {
                            stats.discovered_items += 1;
                            if let Some(metadata) = event.metadata {
                                if let Some(post_id) = metadata.post_id {
                                    stats.discovered_post_ids.insert(post_id);
                                }
                            }
                        }
                        "item_downloaded" => {
                            let Some(file_path) = event.file_path else {
                                continue;
                            };
                            let Some(metadata) = event.metadata else {
                                continue;
                            };
                            log_bridge_item_intake(&metadata);
                            stats.discovered_items += 1;
                            if let Some(post_id) = metadata.post_id.clone() {
                                stats.discovered_post_ids.insert(post_id);
                            }
                            if item_tx
                                .send(DownloadedItem {
                                    file_path: PathBuf::from(file_path),
                                    metadata,
                                })
                                .await
                                .is_err()
                            {
                                tracing::warn!("gallery-dl bridge: receiver dropped, stopping");
                                break;
                            }
                        }
                        "item_skipped_archive" => {
                            stats.skipped_archive_items += 1;
                            if let Some(metadata) = event.metadata {
                                if let Some(post_id) = metadata.post_id {
                                    stats.discovered_post_ids.insert(post_id);
                                }
                            }
                        }
                        "item_failed_final" => {
                            if let Some(metadata) = event.metadata {
                                if let Some(post_id) = metadata.post_id.clone() {
                                    stats.discovered_post_ids.insert(post_id);
                                }
                                stats.failed_items.push(FailedDownloadedItem {
                                    metadata,
                                    error_message: "gallery-dl exhausted item retries".to_string(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            stats
        });

        let stderr_handle = tokio::spawn(async move {
            let mut output = String::new();
            if let Some(err) = child_stderr {
                let mut reader = BufReader::new(err).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if trimmed.contains("[error]") {
                        warn!(line = trimmed, "gallery-dl error");
                    } else if trimmed.contains("[warning]") {
                        info!(line = trimmed, "gallery-dl warning");
                    } else {
                        info!(line = trimmed, "gallery-dl stderr");
                    }

                    output.push_str(&line);
                    output.push('\n');
                }
            }
            output
        });

        let child_pid = child.id();
        info!(pid = ?child_pid, "gallery-dl subprocess spawned");

        let status = tokio::select! {
            _ = opts.cancel.cancelled() => {
                info!(pid = ?child_pid, "Gallery-dl cancelled by user, killing subprocess");
                // On Windows, child.kill() only terminates the direct process, not
                // the tree (gallery-dl may run through a Python wrapper). Use
                // taskkill /F /T to kill the full process tree.
                #[cfg(target_os = "windows")]
                {
                    if let Some(pid) = child_pid {
                        info!(pid, "Windows: killing process tree via taskkill /F /T");
                        use std::os::windows::process::CommandExt;
                        let kill_result = tokio::process::Command::new("taskkill")
                            .args(["/F", "/T", "/PID", &pid.to_string()])
                            .creation_flags(0x08000000)
                            .stdout(Stdio::null())
                            .stderr(Stdio::piped())
                            .output()
                            .await;
                        match &kill_result {
                            Ok(output) => {
                                let code = output.status.code().unwrap_or(-1);
                                if code != 0 {
                                    let stderr = String::from_utf8_lossy(&output.stderr);
                                    warn!(pid, exit_code = code, stderr = %stderr,
                                        "taskkill returned non-zero; falling back to child.kill()");
                                    let _ = child.kill().await;
                                } else {
                                    info!(pid, "taskkill succeeded");
                                }
                            }
                            Err(e) => {
                                warn!(pid, error = %e,
                                    "taskkill failed to execute; falling back to child.kill()");
                                let _ = child.kill().await;
                            }
                        }
                    } else {
                        warn!("gallery-dl child has no PID; falling back to child.kill()");
                        let _ = child.kill().await;
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = child.kill().await;
                }
                // Short timeout on wait — if the process doesn't die in 2s, move on
                match tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    child.wait(),
                ).await {
                    Ok(Ok(s)) => s,
                    Ok(Err(e)) => {
                        warn!(error = %e, "gallery-dl wait failed after kill");
                        std::process::ExitStatus::default()
                    }
                    Err(_) => {
                        warn!("gallery-dl didn't exit within 2s after kill — abandoning");
                        std::process::ExitStatus::default()
                    }
                }
            }
            result = child.wait() => {
                result.map_err(|e| format!("Gallery-dl process error: {e}"))?
            }
        };

        let exit_code = status.code().unwrap_or(-1);
        let bridge_stats = stdout_handle.await.unwrap_or_default();
        let stderr = stderr_handle.await.unwrap_or_default();
        let had_download_errors = !bridge_stats.failed_items.is_empty();

        info!(
            exit_code,
            had_download_errors,
            discovered_items = bridge_stats.discovered_items,
            skipped_archive_items = bridge_stats.skipped_archive_items,
            discovered_posts = bridge_stats.discovered_post_ids.len(),
            elapsed_ms = run_start.elapsed().as_millis(),
            "gallery-dl finished"
        );

        let _ = tokio::fs::remove_file(&config_path).await;

        Ok(RunSummary {
            exit_code,
            had_download_errors,
            stderr_output: stderr,
            temp_dir,
            failed_items: bridge_stats.failed_items,
            discovered_items: bridge_stats.discovered_items,
            discovered_post_ids: bridge_stats.discovered_post_ids.into_iter().collect(),
            skipped_archive_items: bridge_stats.skipped_archive_items,
        })
    }

    fn python_executable(&self) -> String {
        #[cfg(target_os = "windows")]
        {
            "python".to_string()
        }
        #[cfg(not(target_os = "windows"))]
        {
            "python3".to_string()
        }
    }

    fn gallery_dl_module_dir(&self) -> Option<PathBuf> {
        let parent = self.binary_path.parent()?;
        let wheel = parent.join("wheel");
        if wheel.join("gallery_dl").is_dir() {
            return Some(wheel);
        }
        None
    }

    fn bridge_script_path(&self) -> Result<PathBuf, String> {
        let mut roots = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            roots.push(cwd);
        }
        if let Ok(exe) = std::env::current_exe() {
            let mut dir = exe.parent().map(|p| p.to_path_buf());
            while let Some(current) = dir {
                roots.push(current.clone());
                dir = current.parent().map(|p| p.to_path_buf());
                if roots.len() > 8 {
                    break;
                }
            }
        }
        for root in roots {
            let candidate = root.join("scripts").join("gallery_dl_bridge.py");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err("Unable to locate scripts/gallery_dl_bridge.py".to_string())
    }

    async fn ensure_runtime_dependencies(&self) -> Result<(), String> {
        static DEPS_CHECKED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        if DEPS_CHECKED.get().is_some() {
            return Ok(());
        }

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
                DEPS_CHECKED.set(()).ok();
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

        DEPS_CHECKED.set(()).ok();
        Ok(())
    }
}
