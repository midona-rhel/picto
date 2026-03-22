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

mod adapters;

/// Convert a path string to a `PathBuf`, prefixing `\\?\` on Windows to
/// bypass the 260-character MAX_PATH limitation. Without this, `is_file()`
/// and `fs::read()` silently fail on long gallery-dl download paths.
fn to_long_path(raw: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let p = raw.replace('/', "\\");
        if p.starts_with("\\\\?\\") {
            PathBuf::from(p)
        } else {
            PathBuf::from(format!("\\\\?\\{}", p))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from(raw)
    }
}
mod config;
mod failure;
mod filesystem;
mod metadata;
mod metadata_validation;
mod sites;

use std::path::PathBuf;
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::credential_store::SiteCredential;

use self::config::build_config;

pub use failure::{classify_failure, FailureKind};
pub use filesystem::cleanup_temp_dir;
pub use metadata::{extract_creator_identifier, parse_metadata, parse_tags};
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
    /// Max files to download (maps to `--post-range`). None = unlimited.
    pub post_limit: Option<u32>,
    /// Starting post index for `--post-range` (1-based). Used by range_offset pagination.
    pub range_start: u32,
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

/// Summary of a gallery-dl invocation (no items — those are streamed via channel).
pub struct RunSummary {
    pub exit_code: i32,
    pub stderr_output: String,
    pub temp_dir: PathBuf,
    /// True if any individual file download failed after all retries.
    pub had_download_errors: bool,
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
    pub source_urls: Vec<String>,
    pub rating: Option<String>,
    pub title: Option<String>,
    pub post_id: Option<String>,
    pub created_at: Option<String>,
    /// Gallery-dl extractor category (e.g. "danbooru", "pixiv").
    pub category: Option<String>,
    /// 0-based page index within a multi-image post (gallery-dl `num` field).
    pub page_num: Option<u32>,
    /// Total pages in the post (gallery-dl `count` field). >1 means multi-image.
    pub page_count: Option<u32>,
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
        info!(config = %config_json, "gallery-dl config written to {}", config_path.display());
        tokio::fs::write(&config_path, &config_json)
            .await
            .map_err(|e| format!("Config write error: {e}"))?;

        // 3. Build command arguments
        let mut args = vec![
            "--config".to_string(),
            config_path.display().to_string(),
            "--config-ignore".to_string(),
            "--write-metadata".to_string(),
            "--no-input".to_string(),
            "-d".to_string(),
            temp_dir.display().to_string(),
        ];

        if let Some(limit) = opts.post_limit {
            let start = opts.range_start.max(1);
            let end = start.saturating_add(limit).saturating_sub(1);
            args.push("--post-range".to_string());
            args.push(format!("{start}-{end}"));
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
            post_limit = ?opts.post_limit,
            range_start = opts.range_start,
            abort_threshold = ?opts.abort_threshold,
            elapsed_ms = run_start.elapsed().as_millis(),
            "Spawning gallery-dl"
        );
        info!(binary = %self.binary_path.display(), args = ?args, "gallery-dl command");

        // 4. Spawn subprocess
        let mut cmd = tokio::process::Command::new(&self.binary_path);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

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

        // 5. Stream stdout: each line is a downloaded file path.
        //    Parse its sidecar and send through channel for immediate import.
        use tokio::io::{AsyncBufReadExt, BufReader};
        let stdout_handle = tokio::spawn(async move {
            if let Some(out) = child_stdout {
                let mut reader = BufReader::new(out).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    // On Windows, gallery-dl may emit paths with \r or
                    // surrounding quotes — strip both.
                    let trimmed = line.trim().trim_matches('"').trim().to_string();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let path = to_long_path(&trimmed);
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if ext == "json" {
                        tracing::trace!(path = %trimmed, "gallery-dl stdout: skipping json sidecar");
                        continue;
                    }
                    if !path.is_file() {
                        tracing::warn!(path = %trimmed, "gallery-dl stdout: path is not a file, skipping");
                        continue;
                    }

                    tracing::info!(path = %trimmed, ext, "gallery-dl stdout: downloaded file");
                    let metadata = filesystem::parse_sidecar_for_file(&path);
                    tracing::debug!(
                        post_id = metadata.post_id.as_deref().unwrap_or("?"),
                        page_num = ?metadata.page_num,
                        page_count = ?metadata.page_count,
                        tags = metadata.tags.len(),
                        category = metadata.category.as_deref().unwrap_or("?"),
                        "gallery-dl stdout: parsed sidecar metadata"
                    );
                    if item_tx
                        .send(DownloadedItem {
                            file_path: path,
                            metadata,
                        })
                        .await
                        .is_err()
                    {
                        tracing::warn!("gallery-dl stdout: receiver dropped, stopping");
                        break;
                    }
                }
            }
        });

        let download_failed_cancel = CancellationToken::new();
        let download_failed_signal = download_failed_cancel.clone();

        let stderr_handle = tokio::spawn(async move {
            let mut output = String::new();
            let mut had_download_errors = false;
            // Keep recent warning lines to attach as context when a [download][error] fires.
            let mut recent_warnings: Vec<String> = Vec::new();
            if let Some(err) = child_stderr {
                let mut reader = BufReader::new(err).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let trimmed = line.trim();
                    if trimmed.is_empty() { continue; }

                    if trimmed.contains("[download][error]") {
                        had_download_errors = true;
                        let context = if recent_warnings.is_empty() {
                            "no prior warnings".to_string()
                        } else {
                            recent_warnings.join(" | ")
                        };
                        warn!(
                            line = trimmed,
                            context = %context,
                            "gallery-dl: download failed after all retries — stopping query"
                        );
                        recent_warnings.clear();
                        // All retries exhausted for a file — abort the run.
                        download_failed_signal.cancel();
                        break;
                    } else if trimmed.contains("[error]") {
                        warn!(line = trimmed, "gallery-dl error");
                    } else if trimmed.contains("[warning]") {
                        info!(line = trimmed, "gallery-dl warning");
                        recent_warnings.push(trimmed.to_string());
                        if recent_warnings.len() > 10 {
                            recent_warnings.remove(0);
                        }
                    } else {
                        info!(line = trimmed, "gallery-dl stderr");
                    }

                    output.push_str(&line);
                    output.push('\n');
                }
            }
            (output, had_download_errors)
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
            _ = download_failed_cancel.cancelled() => {
                warn!(pid = ?child_pid, "Gallery-dl download failed after all retries — killing subprocess");
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = child.kill().await;
                }
                #[cfg(target_os = "windows")]
                {
                    if let Some(pid) = child_pid {
                        use std::os::windows::process::CommandExt;
                        let _ = tokio::process::Command::new("taskkill")
                            .args(["/F", "/T", "/PID", &pid.to_string()])
                            .creation_flags(0x08000000)
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .output()
                            .await;
                    } else {
                        let _ = child.kill().await;
                    }
                }
                match tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    child.wait(),
                ).await {
                    Ok(Ok(s)) => s,
                    _ => std::process::ExitStatus::default(),
                }
            }
            result = child.wait() => {
                result.map_err(|e| format!("Gallery-dl process error: {e}"))?
            }
        };

        let exit_code = status.code().unwrap_or(-1);
        let _ = stdout_handle.await;
        let (stderr, had_download_errors) = stderr_handle.await.unwrap_or_default();

        info!(
            exit_code,
            had_download_errors,
            elapsed_ms = run_start.elapsed().as_millis(),
            "gallery-dl finished"
        );

        let _ = tokio::fs::remove_file(&config_path).await;

        Ok(RunSummary {
            exit_code,
            had_download_errors,
            stderr_output: stderr,
            temp_dir,
        })
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
