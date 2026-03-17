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
mod config;
mod failure;
mod filesystem;
mod metadata;
mod metadata_validation;
mod sites;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::credential_store::SiteCredential;

use self::config::build_config;
use self::filesystem::scan_output_dir;

pub use failure::{FailureKind, classify_failure};
pub use filesystem::cleanup_temp_dir;
pub use metadata::{extract_creator_identifier, parse_metadata, parse_tags};
pub use metadata_validation::{
    SiteMetadataSchema, SiteMetadataValidationResult, get_site_metadata_schema,
    validate_site_metadata,
};
pub use sites::{
    SITES, SiteEntry, build_url, canonical_site_id, extract_domain, site_by_id, substitute_query,
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
    pub source_urls: Vec<String>,
    pub rating: Option<String>,
    pub title: Option<String>,
    pub post_id: Option<String>,
    pub created_at: Option<String>,
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
