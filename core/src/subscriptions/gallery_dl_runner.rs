//! Gallery-dl bridge runner.
//!
//! Manages gallery-dl invocations through the owned Picto bridge:
//! generates temp config files, spawns the bridge, and consumes NDJSON item
//! events without relying on sidecar JSON as the live contract.
//!
//! Gallery-dl reference: `external/gallery-dl/` (source code).
//! Key source files consulted:
//! - `gallery_dl/option.py` — CLI flag definitions (argparse)
//! - `gallery_dl/job.py` — DownloadJob, skip/abort logic (lines 621-632)
//! - `gallery_dl/archive.py` — SQLite download archive
//! - `gallery_dl/extractor/danbooru.py` — tag_string_* fields

mod adapters;
mod config;
mod failure;
mod filesystem;
mod metadata;
mod sites;

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::subscriptions::source_adapter::{DownloadedItem, FailedDownloadedItem, ParsedMetadata};

use self::config::build_config;

const BRIDGE_INACTIVITY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

pub use failure::{
    classify_failure, error_tail, final_error_line, has_error_lines, FailureKind, RecoveryAction,
};
pub use filesystem::cleanup_temp_dir;
pub use metadata::{extract_creator_identifier, parse_metadata, parse_tags};
pub use sites::{
    build_url, extract_domain, normalize_baraag_username, normalize_ehentai_gallery_url,
    normalize_fanbox_creator, normalize_furaffinity_username, normalize_newgrounds_username,
    normalize_onlyfans_creator, normalize_patreon_creator, normalize_subscribestar_creator,
    normalize_tumblr_blog, normalize_twitter_username, normalize_webtoons_url, site_by_id,
    SiteEntry, SITES,
};

#[derive(Debug, Clone, PartialEq)]
pub struct GalleryDlAuthConfig {
    pub site_category: String,
    pub fragment: serde_json::Value,
}

pub struct RunOptions {
    /// Optional subscription identifier for structured diagnostics.
    pub subscription_id: Option<i64>,
    /// Optional query identifier for structured diagnostics.
    pub query_id: Option<i64>,
    /// Site identifier used to derive the gallery-dl config.
    pub site_id: String,
    /// Full source URL built by the subscription source adapter.
    pub url: String,
    /// Maximum source posts to process. None = unlimited.
    pub post_limit: Option<u32>,
    /// Starting source-post index (1-based). Used by range-offset pagination.
    pub range_start: u32,
    /// Opaque source-owned continuation token. Only used by sources whose
    /// gallery-dl extractor exposes stable keyset pagination.
    pub source_cursor: Option<String>,
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
    pub skipped_archive_items: usize,
    pub source_cursor: Option<String>,
    pub source_page_items: usize,
}

#[derive(Debug, Deserialize)]
struct BridgeEvent {
    event: String,
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    #[serde(default)]
    item_url: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
}

#[derive(Default)]
struct BridgeOutputStats {
    failed_items: Vec<FailedDownloadedItem>,
    discovered_items: usize,
    skipped_archive_items: usize,
    source_cursor: Option<String>,
    source_page_items: usize,
}

enum BridgeLaunch {
    Sidecar(PathBuf),
    #[cfg(debug_assertions)]
    DevelopmentPython {
        python: String,
        script: PathBuf,
        module_dir: PathBuf,
    },
}

impl BridgeLaunch {
    fn module_dir(&self) -> Option<&Path> {
        match self {
            Self::Sidecar(_) => None,
            #[cfg(debug_assertions)]
            Self::DevelopmentPython { module_dir, .. } => Some(module_dir),
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::Sidecar(_) => "self-contained sidecar",
            #[cfg(debug_assertions)]
            Self::DevelopmentPython { .. } => "development Python fallback",
        }
    }
}

fn config_bool_at<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<bool> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_bool()
}

fn bridge_ranges(start: u32, limit: Option<u32>) -> (Option<String>, Option<String>) {
    let range = limit.map(|limit| {
        let start = start.max(1);
        let end = start.saturating_add(limit).saturating_sub(1);
        format!("{start}-{end}")
    });

    (range, None)
}

fn bridge_ranges_for_site(
    site_id: &str,
    start: u32,
    limit: Option<u32>,
) -> (Option<String>, Option<String>) {
    if site_id == "artstation" {
        return (None, None);
    }
    if matches!(site_id, "patreon" | "tumblr") {
        return (None, None);
    }
    if matches!(site_id, "idolcomplex" | "sankaku") {
        return bridge_ranges(1, limit);
    }
    if site_id == "deviantart" {
        // The bridge pages whole deviations before expanding each one into
        // child media and emits the authoritative source cursor.
        return (None, None);
    }
    if site_id == "webtoons" {
        let child_range = limit.map(|limit| {
            let start = start.max(1);
            let end = start.saturating_add(limit).saturating_sub(1);
            format!("{start}-{end}")
        });
        return (None, child_range);
    }
    bridge_ranges(start, limit)
}

fn metadata_has_raw_key(metadata: &ParsedMetadata, key: &str) -> bool {
    metadata
        .raw_metadata
        .as_ref()
        .and_then(|raw| raw.get(key))
        .is_some()
}

fn log_bridge_item_intake(metadata: &ParsedMetadata) {
    let mut summary = [0usize; 7];
    for (namespace, _) in &metadata.tags {
        summary[0] += 1;
        summary[match namespace.as_str() {
            "creator" => 1,
            "character" => 2,
            "series" => 3,
            "" | "general" => 4,
            "meta" => 5,
            _ => 6,
        }] += 1;
    }
    debug!(
        post_id = metadata.post_id.as_deref().unwrap_or("?"),
        category = metadata.category.as_deref().unwrap_or("?"),
        item_key = metadata.item_key.as_deref().unwrap_or("?"),
        raw_has_tags_artist = metadata_has_raw_key(metadata, "tags_artist"),
        raw_has_tags_character = metadata_has_raw_key(metadata, "tags_character"),
        raw_has_tags_copyright = metadata_has_raw_key(metadata, "tags_copyright"),
        parsed_tag_count = summary[0],
        creator_tag_count = summary[1],
        character_tag_count = summary[2],
        series_tag_count = summary[3],
        general_tag_count = summary[4],
        meta_tag_count = summary[5],
        other_namespaced_tag_count = summary[6],
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
        post_tx: Option<tokio::sync::mpsc::Sender<ParsedMetadata>>,
    ) -> Result<RunSummary, String> {
        let run_start = std::time::Instant::now();
        let launch = self.launch_spec()?;
        info!(
            runtime = launch.description(),
            "gallery-dl bridge runtime selected"
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

        let (post_range, child_range) =
            bridge_ranges_for_site(&opts.site_id, opts.range_start, opts.post_limit);
        let bridge_request = serde_json::json!({
            "url": opts.url,
            "site_id": opts.site_id,
            "subscription_id": opts.subscription_id,
            "query_id": opts.query_id,
            "config_path": config_path.display().to_string(),
            "gallery_dl_module_dir": launch.module_dir().map(|path| path.display().to_string()),
            "post_range": post_range,
            "child_range": child_range,
            "post_limit": opts.post_limit,
            "range_start": opts.range_start,
            "source_cursor": opts.source_cursor,
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

        info!(
            url = %opts.url,
            post_limit = ?opts.post_limit,
            range_start = opts.range_start,
            abort_threshold = ?opts.abort_threshold,
            elapsed_ms = run_start.elapsed().as_millis(),
            "Spawning gallery-dl bridge"
        );
        match &launch {
            BridgeLaunch::Sidecar(path) => {
                info!(bridge = %path.display(), "gallery-dl bridge command");
            }
            #[cfg(debug_assertions)]
            BridgeLaunch::DevelopmentPython { python, script, .. } => {
                info!(python = %python, bridge = %script.display(), "gallery-dl bridge command");
            }
        }

        // 4. Spawn subprocess
        let mut cmd = match &launch {
            BridgeLaunch::Sidecar(path) => tokio::process::Command::new(path),
            #[cfg(debug_assertions)]
            BridgeLaunch::DevelopmentPython {
                python,
                script,
                module_dir,
            } => {
                let mut cmd = tokio::process::Command::new(python);
                cmd.arg(script);

                let mut python_paths = vec![module_dir.clone()];
                if let Some(existing) = std::env::var_os("PYTHONPATH") {
                    python_paths.extend(std::env::split_paths(&existing));
                }
                let python_path = std::env::join_paths(python_paths)
                    .map_err(|_| "Failed to construct development PYTHONPATH".to_string())?;
                cmd.env("PYTHONPATH", python_path);
                cmd
            }
        };
        cmd.arg("--request")
            .arg(&request_path)
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

        // 5. Stream stdout: bridge emits NDJSON events.
        use tokio::io::{AsyncBufReadExt, BufReader};
        let (progress_tx, mut progress_rx) = tokio::sync::watch::channel(std::time::Instant::now());
        let stdout_progress = progress_tx.clone();
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
                    if matches!(
                        event.event.as_str(),
                        "item_discovered"
                            | "post_traversed"
                            | "item_downloaded"
                            | "item_skipped_archive"
                            | "item_failed_final"
                            | "source_cursor"
                    ) {
                        stdout_progress.send_replace(std::time::Instant::now());
                    }
                    let metadata = event.metadata.as_ref().map(|raw| {
                        metadata::parse_metadata_with_url(raw, event.item_url.as_deref())
                    });
                    match event.event.as_str() {
                        "item_discovered" => {
                            stats.discovered_items += 1;
                        }
                        "post_traversed" => {
                            stats.source_page_items += 1;
                            if let (Some(post_tx), Some(metadata)) = (&post_tx, metadata) {
                                if post_tx.send(metadata).await.is_err() {
                                    tracing::warn!("gallery-dl bridge: post receiver dropped");
                                }
                            }
                        }
                        "item_downloaded" => {
                            let Some(file_path) = event.file_path else {
                                continue;
                            };
                            let Some(metadata) = metadata else {
                                continue;
                            };
                            log_bridge_item_intake(&metadata);
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
                        }
                        "item_failed_final" => {
                            if let Some(metadata) = metadata {
                                let item_url =
                                    event.item_url.as_deref().unwrap_or("unknown media URL");
                                let error_message = event
                                    .error_message
                                    .filter(|message| !message.trim().is_empty())
                                    .unwrap_or_else(|| format!("Could not download {item_url}"));
                                stats.failed_items.push(FailedDownloadedItem {
                                    metadata,
                                    error_message,
                                });
                            }
                        }
                        "source_cursor" => {
                            stats.source_cursor = event.cursor;
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

        let inactivity = async move {
            loop {
                let deadline = *progress_rx.borrow() + BRIDGE_INACTIVITY_TIMEOUT;
                tokio::select! {
                    _ = tokio::time::sleep_until(deadline.into()) => {
                        if progress_rx.borrow().elapsed() >= BRIDGE_INACTIVITY_TIMEOUT {
                            break;
                        }
                    }
                    changed = progress_rx.changed() => {
                        if changed.is_err() {
                            std::future::pending::<()>().await;
                        }
                    }
                }
            }
        };
        tokio::pin!(inactivity);

        let (status, stalled) = tokio::select! {
            _ = opts.cancel.cancelled() => {
                info!(pid = ?child_pid, "Gallery-dl cancelled by user, killing subprocess");
                // On Windows, child.kill() only terminates the direct process, not
                // the tree (the development fallback may run through Python). Use
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
                let status = match tokio::time::timeout(
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
                };
                (status, false)
            }
            result = child.wait() => {
                (result.map_err(|e| format!("Gallery-dl process error: {e}"))?, false)
            }
            _ = &mut inactivity => {
                warn!(pid = ?child_pid, "gallery-dl bridge made no progress; killing subprocess");
                let _ = child.kill().await;
                let status = match tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    child.wait(),
                ).await {
                    Ok(Ok(status)) => status,
                    _ => std::process::ExitStatus::default(),
                };
                (status, true)
            }
        };

        let exit_code = status.code().unwrap_or(-1);
        let bridge_stats = stdout_handle.await.unwrap_or_default();
        let stderr = stderr_handle.await.unwrap_or_default();
        if stalled {
            return Err(format!(
                "gallery-dl made no progress for {} seconds{}",
                BRIDGE_INACTIVITY_TIMEOUT.as_secs(),
                final_error_line(&stderr)
                    .map(|line| format!(": {line}"))
                    .unwrap_or_default()
            ));
        }
        let had_download_errors = !bridge_stats.failed_items.is_empty();

        info!(
            exit_code,
            had_download_errors,
            discovered_items = bridge_stats.discovered_items,
            skipped_archive_items = bridge_stats.skipped_archive_items,
            traversed_posts = bridge_stats.source_page_items,
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
            skipped_archive_items: bridge_stats.skipped_archive_items,
            source_cursor: bridge_stats.source_cursor,
            source_page_items: bridge_stats.source_page_items,
        })
    }

    fn launch_spec(&self) -> Result<BridgeLaunch, String> {
        if is_sidecar_path(&self.binary_path) {
            return Ok(BridgeLaunch::Sidecar(self.binary_path.clone()));
        }

        #[cfg(debug_assertions)]
        {
            if !is_development_fallback_path(&self.binary_path) {
                return Err(format!(
                    "Unsupported gallery-dl runtime `{}`; expected the Picto bridge sidecar",
                    self.binary_path.display()
                ));
            }

            let module_dir = self.gallery_dl_module_dir().ok_or_else(|| {
                "Development gallery-dl fallback is missing its vendored Python wheel".to_string()
            })?;
            let script = self.bridge_script_path()?;
            return Ok(BridgeLaunch::DevelopmentPython {
                python: self.python_executable(),
                script,
                module_dir,
            });
        }

        #[cfg(not(debug_assertions))]
        {
            Err(format!(
                "Unsupported gallery-dl runtime `{}`; packaged builds require the Picto bridge sidecar",
                self.binary_path.display()
            ))
        }
    }

    #[cfg(debug_assertions)]
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

    #[cfg(debug_assertions)]
    fn gallery_dl_module_dir(&self) -> Option<PathBuf> {
        let parent = self.binary_path.parent()?;
        let wheel = parent.join("wheel");
        if wheel.join("gallery_dl").is_dir() {
            return Some(wheel);
        }
        None
    }

    #[cfg(debug_assertions)]
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
}

#[cfg(target_os = "windows")]
const BRIDGE_BIN: &str = "picto-gallery-dl-bridge.exe";
#[cfg(not(target_os = "windows"))]
const BRIDGE_BIN: &str = "picto-gallery-dl-bridge";

#[cfg(all(debug_assertions, target_os = "windows"))]
const DEV_FALLBACK_BIN: &str = "gallery-dl.exe";
#[cfg(all(debug_assertions, not(target_os = "windows")))]
const DEV_FALLBACK_BIN: &str = "gallery-dl";

fn is_sidecar_path(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some(BRIDGE_BIN)
}

#[cfg(debug_assertions)]
fn is_development_fallback_path(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some(DEV_FALLBACK_BIN)
        && path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            == Some("gallery-dl")
        && path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            == Some("vendor")
}

#[cfg(test)]
mod tests {
    use super::{
        bridge_ranges, bridge_ranges_for_site, is_sidecar_path, BridgeLaunch, GalleryDlRunner,
    };

    #[test]
    fn runner_uses_direct_sidecar_launch() {
        #[cfg(target_os = "windows")]
        let path = std::path::PathBuf::from("vendor/gallery-dl/picto-gallery-dl-bridge.exe");
        #[cfg(not(target_os = "windows"))]
        let path = std::path::PathBuf::from("vendor/gallery-dl/picto-gallery-dl-bridge");

        let runner = GalleryDlRunner::new(path.clone());
        match runner.launch_spec().unwrap() {
            BridgeLaunch::Sidecar(selected) => assert_eq!(selected, path),
            #[cfg(debug_assertions)]
            BridgeLaunch::DevelopmentPython { .. } => panic!("sidecar selected Python fallback"),
        }
    }

    #[test]
    fn picto_bridge_path_is_selected_as_a_sidecar() {
        #[cfg(target_os = "windows")]
        let path = std::path::Path::new("vendor/gallery-dl/picto-gallery-dl-bridge.exe");
        #[cfg(not(target_os = "windows"))]
        let path = std::path::Path::new("vendor/gallery-dl/picto-gallery-dl-bridge");

        assert!(is_sidecar_path(path));
        assert!(!is_sidecar_path(std::path::Path::new(
            "vendor/gallery-dl/gallery-dl"
        )));
    }

    #[test]
    fn post_limit_maps_to_one_gallery_dl_post_range() {
        assert_eq!(
            bridge_ranges(101, Some(50)),
            (Some("101-150".to_string()), None)
        );
    }

    #[test]
    fn artstation_uses_native_project_limit_instead_of_asset_range() {
        assert_eq!(
            bridge_ranges_for_site("artstation", 5, Some(2)),
            (None, None)
        );
        assert_eq!(
            bridge_ranges_for_site("danbooru", 5, Some(2)),
            (Some("5-6".to_string()), None)
        );
        assert_eq!(
            bridge_ranges_for_site("idolcomplex", 5, Some(2)),
            (Some("1-2".to_string()), None)
        );
        assert_eq!(
            bridge_ranges_for_site("sankaku", 5, Some(2)),
            (Some("1-2".to_string()), None)
        );
    }

    #[test]
    fn webtoons_uses_child_range_without_splitting_episode_images() {
        assert_eq!(
            bridge_ranges_for_site("webtoons", 5, Some(2)),
            (None, Some("5-6".to_string()))
        );
        assert_eq!(bridge_ranges_for_site("webtoons", 1, None), (None, None));
    }

    #[test]
    fn deviantart_pages_whole_source_posts_in_the_bridge() {
        assert_eq!(
            bridge_ranges_for_site("deviantart", 5, Some(2)),
            (None, None)
        );
        assert_eq!(bridge_ranges_for_site("deviantart", 1, None), (None, None));
    }

    #[test]
    fn tumblr_uses_native_whole_post_limits() {
        assert_eq!(bridge_ranges_for_site("tumblr", 5, Some(2)), (None, None));
        assert_eq!(bridge_ranges_for_site("tumblr", 1, None), (None, None));
    }

    #[test]
    fn patreon_uses_native_cursor_batches() {
        assert_eq!(
            bridge_ranges_for_site("patreon", 101, Some(100)),
            (None, None)
        );
    }
}
