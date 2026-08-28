//! Offline source runner used only by the guided-tour library.

use std::path::{Path, PathBuf};

use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::subscription_runtime_v2::{
    DownloadedItem, RunnerFailure, RunnerFailureKind, RunnerFuture, RunnerSuccess, SourceEvent,
    SourceRunner,
};
use crate::subscriptions_v2::{ClaimedQueryRun, NormalizedItem, NormalizedPost};
use picto_library::{ImmutableMediaFacts, Lifecycle, PreparedImport, Rating, SourceIdentity};

#[derive(Clone)]
pub struct TutorialSourceRunner {
    fixture_root: PathBuf,
}

impl TutorialSourceRunner {
    pub fn new(fixture_root: PathBuf) -> Self {
        Self { fixture_root }
    }
}

impl SourceRunner for TutorialSourceRunner {
    fn run<'a>(
        &'a self,
        query: &'a ClaimedQueryRun,
        output: Sender<SourceEvent>,
        cancel: CancellationToken,
    ) -> RunnerFuture<'a> {
        Box::pin(async move {
            if query.site_id != "twitter" || query.query_text != "LeonardoDaVinci" {
                return Err(RunnerFailure::terminal(
                    RunnerFailureKind::InvalidQuery,
                    "The guided tour only supports its bundled Leonardo archive query",
                ));
            }
            let fixtures = if query.initial_run_complete {
                vec![
                    ("lady-with-an-ermine.jpg", "lady-with-an-ermine", 0_i64),
                    (
                        "lady-with-an-ermine-detail.jpg",
                        "lady-with-an-ermine-detail",
                        1_i64,
                    ),
                ]
            } else {
                vec![("mona-lisa.jpg", "mona-lisa", 0_i64)]
            };
            let post_key = if query.initial_run_complete {
                "tutorial-lady-with-an-ermine-post"
            } else {
                "tutorial-mona-lisa-post"
            };
            let post = normalized_post(post_key, &fixtures);
            send(&output, SourceEvent::PostTraversed(post.clone()), &cancel).await?;

            for (index, (file_name, item_key, _position)) in fixtures.iter().enumerate() {
                if cancel.is_cancelled() {
                    return Err(RunnerFailure::terminal(
                        RunnerFailureKind::Interrupted,
                        "Guided tour subscription cancelled",
                    ));
                }
                let path = self.fixture_root.join(file_name);
                ensure_fixture(&path)?;
                let input = prepare_fixture(&path, post_key, item_key, file_name).await?;
                send(
                    &output,
                    SourceEvent::MediaDownloaded(DownloadedItem {
                        post: post.clone(),
                        input,
                        post_complete: index + 1 == fixtures.len(),
                        force_collection: false,
                        delete_after_ingest: false,
                    }),
                    &cancel,
                )
                .await?;
                tokio::time::sleep(std::time::Duration::from_millis(180)).await;
            }
            Ok(RunnerSuccess {
                resume_cursor: Some(
                    if query.initial_run_complete {
                        "grouped"
                    } else {
                        "single"
                    }
                    .into(),
                ),
                cleanup_paths: Vec::new(),
            })
        })
    }
}

async fn prepare_fixture(
    path: &Path,
    post_key: &str,
    item_key: &str,
    file_name: &str,
) -> Result<PreparedImport, RunnerFailure> {
    let prepared = crate::media_processing::PreparedMediaSource::prepare_ingest(path)
        .await
        .map_err(|error| {
            RunnerFailure::terminal(RunnerFailureKind::InvalidOutput, error.to_string())
        })?;
    let hash_path = path.to_path_buf();
    let content_hash = tokio::task::spawn_blocking(move || {
        crate::media_processing::get_hash_from_path(&hash_path).map(hex::encode)
    })
    .await
    .map_err(|error| RunnerFailure::terminal(RunnerFailureKind::Runtime, error.to_string()))?
    .map_err(|error| {
        RunnerFailure::terminal(RunnerFailureKind::InvalidOutput, error.to_string())
    })?;
    let size_bytes = std::fs::metadata(path)
        .map_err(|error| {
            RunnerFailure::terminal(RunnerFailureKind::InvalidOutput, error.to_string())
        })?
        .len();
    Ok(PreparedImport {
        stable_key: format!("source:twitter:{post_key}:{item_key}"),
        media_name: path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Untitled")
            .into(),
        file_path: path.to_string_lossy().into_owned(),
        facts: ImmutableMediaFacts {
            mime: prepared.mime_type,
            size_bytes,
            width: prepared.pixel_width,
            height: prepared.pixel_height,
            duration_ms: prepared.duration_ms,
            frame_count: prepared.num_frames,
            content_hash,
            perceptual_hash: None,
            palette: Vec::new(),
        },
        lifecycle: Lifecycle::Inbox,
        rating: Rating::Unrated,
        notes: Some("Bundled public-domain guided-tour fixture".into()),
        tags: vec![
            "creator:leonardo da vinci".into(),
            "general:tutorial".into(),
        ],
        folders: Vec::new(),
        source_urls: vec![format!(
            "https://commons.wikimedia.org/wiki/File:{file_name}"
        )],
        source_identity: Some(SourceIdentity {
            source_key: format!("twitter:{post_key}"),
            source_item_key: item_key.into(),
            source_text: Some("{\"tutorial\":true,\"network\":false}".into()),
        }),
        imported_at_ms: chrono::Utc::now().timestamp_millis(),
        captured_at_ms: Some(-14_221_440_000_000),
    })
}

fn normalized_post(post_key: &str, fixtures: &[(&str, &str, i64)]) -> NormalizedPost {
    NormalizedPost {
        site_id: "twitter".into(),
        post_key: post_key.into(),
        canonical_url: Some(format!("https://x.com/leonardo_archive/status/{post_key}")),
        creator_name: Some("Leonardo da Vinci Archive".into()),
        title: Some(
            if fixtures.len() > 1 {
                "Lady with an Ermine studies"
            } else {
                "Mona Lisa"
            }
            .into(),
        ),
        description: Some("An offline guided-tour post using bundled artwork".into()),
        captured_at: Some("1519-05-02T00:00:00Z".into()),
        metadata_json: Some("{\"tutorial\":true,\"network\":false}".into()),
        items: fixtures
            .iter()
            .map(|(file_name, item_key, position)| NormalizedItem {
                item_key: (*item_key).into(),
                position: *position,
                media_url: None,
                canonical_url: Some(format!(
                    "https://commons.wikimedia.org/wiki/File:{file_name}"
                )),
            })
            .collect(),
    }
}

async fn send(
    output: &Sender<SourceEvent>,
    event: SourceEvent,
    cancel: &CancellationToken,
) -> Result<(), RunnerFailure> {
    tokio::select! {
        _ = cancel.cancelled() => Err(RunnerFailure::terminal(
            RunnerFailureKind::Interrupted,
            "Guided tour subscription cancelled",
        )),
        result = output.send(event) => result.map_err(|_| RunnerFailure::terminal(
            RunnerFailureKind::Runtime,
            "Guided tour subscription worker closed",
        )),
    }
}

fn ensure_fixture(path: &Path) -> Result<(), RunnerFailure> {
    if path.is_file() {
        Ok(())
    } else {
        Err(RunnerFailure::terminal(
            RunnerFailureKind::InvalidOutput,
            format!("Missing guided-tour fixture: {}", path.display()),
        ))
    }
}
