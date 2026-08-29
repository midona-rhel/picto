//! OnlyFans extraction through OF-Scraper, normalized into Picto's subscription path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::gallery_dl_source::{normalize_downloads, GalleryDlSourceRunner};
use crate::subscription_runtime::{
    DownloadedItem, RunnerFailure, RunnerFailureKind, RunnerFuture, RunnerSuccess, SourceEvent,
    SourceRunner,
};
use crate::subscriptions::source_adapter::ParsedMetadata;
use crate::subscriptions::{ClaimedQueryRun, NormalizedPost};

const BRIDGE_INACTIVITY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

pub struct OnlyFansSourceRunner {
    library_root: PathBuf,
    binary_override: Option<PathBuf>,
}

impl OnlyFansSourceRunner {
    pub fn open(library_root: &Path) -> Self {
        Self {
            library_root: library_root.to_path_buf(),
            binary_override: None,
        }
    }

    #[cfg(test)]
    pub fn new(library_root: PathBuf, binary: PathBuf) -> Self {
        Self {
            library_root,
            binary_override: Some(binary),
        }
    }

    async fn execute(
        &self,
        query: &ClaimedQueryRun,
        output: mpsc::Sender<SourceEvent>,
        cancel: CancellationToken,
    ) -> Result<RunnerSuccess, RunnerFailure> {
        let credential = crate::credential_store::get_credential("onlyfans")
            .map_err(|error| RunnerFailure::terminal(RunnerFailureKind::Authentication, error))?
            .ok_or_else(|| {
                RunnerFailure::terminal(
                    RunnerFailureKind::Authentication,
                    "OnlyFans requires a connected account",
                )
            })?;
        let cookies = credential.cookies.unwrap_or_default();
        let headers = credential.headers.unwrap_or_default();
        for key in ["sess", "auth_id"] {
            if !cookies.contains_key(key) {
                return Err(RunnerFailure::terminal(
                    RunnerFailureKind::Authentication,
                    format!("OnlyFans login did not provide `{key}`"),
                ));
            }
        }
        for key in ["x-bc", "user-agent"] {
            if !headers.contains_key(key) {
                return Err(RunnerFailure::terminal(
                    RunnerFailureKind::Authentication,
                    format!("OnlyFans login did not provide `{key}`"),
                ));
            }
        }

        let binary = self
            .binary_override
            .clone()
            .map(Ok)
            .unwrap_or_else(|| crate::media_processing::onlyfans_path::onlyfans_path().cloned())
            .map_err(|error| RunnerFailure::terminal(RunnerFailureKind::Runtime, error))?;
        let temp = tempfile::tempdir().map_err(runtime_failure)?;
        let state_dir = query_state_dir(&self.library_root, query.subscription_id, query.query_id);
        std::fs::create_dir_all(&state_dir).map_err(runtime_failure)?;
        let output_dir = state_dir
            .join("downloads")
            .join(format!("run-query-{}", query.run_query_id));
        std::fs::create_dir_all(&output_dir).map_err(runtime_failure)?;
        let request_path = temp.path().join("request.json");
        let request = BridgeRequest {
            parent_pid: std::process::id(),
            state_dir,
            output_dir,
            creator: query.query_text.clone(),
            post_limit: query_limit(),
            before: query
                .resume_cursor
                .clone()
                .filter(|value| !value.is_empty()),
            history_complete: query.resume_cursor.as_deref() == Some(""),
            cookies,
            headers,
        };
        std::fs::write(
            &request_path,
            serde_json::to_vec_pretty(&request).map_err(runtime_failure)?,
        )
        .map_err(runtime_failure)?;

        let mut command = Command::new(binary);
        command
            .arg("--request")
            .arg(&request_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        configure_media_path(&mut command)?;
        let mut child = command.spawn().map_err(runtime_failure)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| runtime_failure("OnlyFans bridge has no stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| runtime_failure("OnlyFans bridge has no stderr"))?;
        let stderr_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut tail = String::new();
            while let Ok(Some(line)) = lines.next_line().await {
                tail = line;
            }
            tail
        });
        let mut lines = BufReader::new(stdout).lines();
        let mut pending: Option<DownloadedItem> = None;
        let mut summary = None;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = child.kill().await;
                    return Err(RunnerFailure::retryable(RunnerFailureKind::Interrupted, "OnlyFans run cancelled"));
                }
                line = tokio::time::timeout(BRIDGE_INACTIVITY_TIMEOUT, lines.next_line()) => match line {
                    Err(_) => {
                        let _ = child.kill().await;
                        return Err(RunnerFailure::retryable(
                            RunnerFailureKind::Network,
                            format!(
                                "OnlyFans made no progress for {} seconds",
                                BRIDGE_INACTIVITY_TIMEOUT.as_secs()
                            ),
                        ));
                    }
                    Ok(line) => match line.map_err(runtime_failure)? {
                    Some(line) => {
                        let Ok(event) = serde_json::from_str::<BridgeEvent>(&line) else { continue };
                        match event {
                            BridgeEvent::Item { item } => {
                                let (file_path, metadata) = item.into_parts();
                                for mut normalized in normalize_downloads("onlyfans", file_path, metadata).await? {
                                    normalized.delete_after_ingest = false;
                                    if pending.as_ref().is_some_and(|previous| previous.post.post_key != normalized.post.post_key) {
                                        return Err(runtime_failure("OnlyFans started a new post before completing the previous post"));
                                    }
                                    if let Some(mut previous) = pending.replace(normalized) {
                                        previous.post_complete = false;
                                        output.send(SourceEvent::MediaDownloaded(previous)).await.map_err(|_| runtime_failure("subscription receiver closed"))?;
                                    }
                                }
                            }
                            BridgeEvent::PostComplete { post_id } => {
                                publish_completed_post(&output, &mut pending, &post_id).await?;
                            }
                            BridgeEvent::PostTraversed { post } => {
                                output.send(SourceEvent::PostTraversed(post.into_post())).await
                                    .map_err(|_| runtime_failure("subscription receiver closed"))?;
                            }
                            BridgeEvent::Summary { next_before, history_complete } => {
                                summary = Some(BridgeSummary { next_before, history_complete });
                            }
                            BridgeEvent::Progress {} => {}
                            BridgeEvent::Error { kind, message } => return Err(classify_bridge_error(&kind, message)),
                        }
                    }
                    None => break,
                    }
                },
            }
        }
        let status = child.wait().await.map_err(runtime_failure)?;
        if !status.success() {
            let message = stderr_task.await.unwrap_or_default();
            return Err(classify_bridge_error("runtime", message));
        }
        let _ = stderr_task.await;
        if pending.is_some() {
            return Err(runtime_failure(
                "OnlyFans bridge ended before completing its final post",
            ));
        }
        let summary =
            summary.ok_or_else(|| runtime_failure("OnlyFans bridge returned no summary"))?;
        let resume_cursor = if summary.history_complete {
            String::new()
        } else {
            summary.next_before.ok_or_else(|| {
                runtime_failure("OnlyFans returned a full page without a pagination cursor")
            })?
        };
        Ok(RunnerSuccess {
            resume_cursor: Some(resume_cursor),
            cleanup_paths: vec![request.output_dir],
        })
    }
}

async fn publish_completed_post(
    output: &mpsc::Sender<SourceEvent>,
    pending: &mut Option<DownloadedItem>,
    post_id: &str,
) -> Result<(), RunnerFailure> {
    if let Some(mut item) = pending.take() {
        if item.post.post_key != post_id {
            return Err(runtime_failure(
                "OnlyFans completed a post that does not match its downloaded media",
            ));
        }
        item.post_complete = true;
        output
            .send(SourceEvent::MediaDownloaded(item))
            .await
            .map_err(|_| runtime_failure("subscription receiver closed"))?;
    }
    let (acknowledge, acknowledged) = tokio::sync::oneshot::channel();
    output
        .send(SourceEvent::PostComplete(acknowledge))
        .await
        .map_err(|_| runtime_failure("subscription receiver closed"))?;
    acknowledged
        .await
        .map_err(|_| runtime_failure("subscription post settlement was not acknowledged"))
}

impl SourceRunner for OnlyFansSourceRunner {
    fn run<'a>(
        &'a self,
        query: &'a ClaimedQueryRun,
        output: mpsc::Sender<SourceEvent>,
        cancel: CancellationToken,
    ) -> RunnerFuture<'a> {
        Box::pin(async move { self.execute(query, output, cancel).await })
    }
}

pub struct SubscriptionSourceRouter {
    gallery_dl: GalleryDlSourceRunner,
    onlyfans: OnlyFansSourceRunner,
}

impl SubscriptionSourceRouter {
    pub fn open(library_root: &Path) -> Self {
        Self {
            gallery_dl: GalleryDlSourceRunner::open(library_root),
            onlyfans: OnlyFansSourceRunner::open(library_root),
        }
    }
}

impl SourceRunner for SubscriptionSourceRouter {
    fn run<'a>(
        &'a self,
        query: &'a ClaimedQueryRun,
        output: mpsc::Sender<SourceEvent>,
        cancel: CancellationToken,
    ) -> RunnerFuture<'a> {
        if query.site_id == "onlyfans" {
            self.onlyfans.run(query, output, cancel)
        } else {
            self.gallery_dl.run(query, output, cancel)
        }
    }
}

pub fn clear_subscription_state(library_root: &Path, subscription_id: i64) -> Result<(), String> {
    let path = library_root
        .join("source-runners")
        .join("onlyfans")
        .join(format!("subscription-{subscription_id}"));
    if path.exists() {
        std::fs::remove_dir_all(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn query_state_dir(root: &Path, subscription_id: i64, query_id: i64) -> PathBuf {
    root.join("source-runners")
        .join("onlyfans")
        .join(format!("subscription-{subscription_id}"))
        .join(format!("query-{query_id}"))
}

fn query_limit() -> i64 {
    1
}

#[derive(Serialize)]
struct BridgeRequest {
    parent_pid: u32,
    state_dir: PathBuf,
    output_dir: PathBuf,
    creator: String,
    post_limit: i64,
    before: Option<String>,
    history_complete: bool,
    cookies: HashMap<String, String>,
    headers: HashMap<String, String>,
}

#[derive(Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum BridgeEvent {
    Item {
        #[serde(flatten)]
        item: BridgeItem,
    },
    PostTraversed {
        #[serde(flatten)]
        post: BridgePost,
    },
    PostComplete {
        post_id: String,
    },
    Summary {
        next_before: Option<String>,
        history_complete: bool,
    },
    Progress {},
    Error {
        kind: String,
        message: String,
    },
}

#[derive(Deserialize)]
struct BridgeSummary {
    next_before: Option<String>,
    history_complete: bool,
}

#[derive(Deserialize)]
struct BridgePost {
    post_id: String,
    post_url: String,
    creator: String,
    title: Option<String>,
    description: Option<String>,
    created_at: Option<String>,
    accessible: bool,
}

impl BridgePost {
    fn into_post(self) -> NormalizedPost {
        NormalizedPost {
            site_id: "onlyfans".to_string(),
            post_key: self.post_id,
            canonical_url: Some(self.post_url),
            creator_name: Some(self.creator),
            title: self.title,
            description: self.description,
            captured_at: self.created_at,
            metadata_json: Some(serde_json::json!({ "accessible": self.accessible }).to_string()),
            items: Vec::new(),
        }
    }
}

#[derive(Deserialize)]
struct BridgeItem {
    file_path: PathBuf,
    post_id: String,
    item_key: String,
    position: u32,
    page_count: u32,
    creator: String,
    title: Option<String>,
    description: Option<String>,
    created_at: Option<String>,
    post_url: String,
    media_url: Option<String>,
    raw_metadata: serde_json::Value,
}

impl BridgeItem {
    fn into_parts(self) -> (PathBuf, ParsedMetadata) {
        let creator = self.creator.trim().to_string();
        let metadata = ParsedMetadata {
            tags: if creator.is_empty() {
                Vec::new()
            } else {
                vec![("creator".to_string(), creator)]
            },
            description: self.description,
            source_url: Some(self.post_url.clone()),
            source_urls: vec![self.post_url.clone()],
            media_url: self.media_url,
            title: self.title,
            post_id: Some(self.post_id),
            created_at: self.created_at,
            page_num: Some(self.position),
            page_count: Some(self.page_count),
            canonical_post_url: Some(self.post_url),
            item_key: Some(self.item_key),
            raw_metadata: Some(self.raw_metadata),
            ..ParsedMetadata::default()
        };
        (self.file_path, metadata)
    }
}

fn runtime_failure(error: impl ToString) -> RunnerFailure {
    RunnerFailure::terminal(RunnerFailureKind::Runtime, error.to_string())
}

fn configure_media_path(command: &mut Command) -> Result<(), RunnerFailure> {
    let Ok(ffmpeg) = crate::media_processing::ffmpeg_path::ffmpeg_path() else {
        return Ok(());
    };
    let Some(directory) = ffmpeg.parent() else {
        return Ok(());
    };
    let mut paths = vec![directory.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    let joined = std::env::join_paths(paths).map_err(runtime_failure)?;
    command.env("PATH", joined);
    Ok(())
}

fn classify_bridge_error(kind: &str, message: String) -> RunnerFailure {
    let failure_kind = match kind {
        "authentication" => RunnerFailureKind::Authentication,
        "network" => RunnerFailureKind::Network,
        "rate_limited" => RunnerFailureKind::RateLimited,
        "invalid_query" => RunnerFailureKind::InvalidQuery,
        "download" => RunnerFailureKind::Download,
        _ => RunnerFailureKind::Runtime,
    };
    if matches!(
        failure_kind,
        RunnerFailureKind::Network | RunnerFailureKind::RateLimited | RunnerFailureKind::Download
    ) {
        RunnerFailure::retryable(failure_kind, message)
    } else {
        RunnerFailure::terminal(failure_kind, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onlyfans_uses_its_own_single_post_process_window() {
        assert_eq!(query_limit(), 1);
    }

    #[test]
    fn bridge_item_normalizes_creator_as_a_tag() {
        let item = BridgeItem {
            file_path: PathBuf::from("media.jpg"),
            post_id: "post-1".to_string(),
            item_key: "media-1".to_string(),
            position: 1,
            page_count: 1,
            creator: " alice ".to_string(),
            title: None,
            description: None,
            created_at: None,
            post_url: "https://onlyfans.com/post-1/alice".to_string(),
            media_url: None,
            raw_metadata: serde_json::json!({}),
        };

        let (_, metadata) = item.into_parts();

        assert_eq!(
            metadata.tags,
            vec![("creator".to_string(), "alice".to_string())]
        );
    }

    #[tokio::test]
    async fn post_completion_waits_for_canonical_settlement() {
        let (output, mut input) = mpsc::channel(1);
        let task =
            tokio::spawn(async move { publish_completed_post(&output, &mut None, "post-1").await });
        let SourceEvent::PostComplete(acknowledge) = input.recv().await.unwrap() else {
            panic!("OnlyFans did not publish the post settlement boundary");
        };
        assert!(!task.is_finished());

        acknowledge.send(()).unwrap();

        task.await.unwrap().unwrap();
    }
}
