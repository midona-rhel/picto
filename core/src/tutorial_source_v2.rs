//! Offline source runner used only by the guided-tour library.

use std::path::{Path, PathBuf};

use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::app::Lifecycle;
use crate::ingest_v2::SourcePostInput;
use crate::subscription_runtime_v2::{
    DownloadedItem, RunnerFailure, RunnerFailureKind, RunnerFuture, RunnerSuccess, SourceEvent,
    SourceRunner,
};
use crate::subscriptions_v2::{ClaimedQueryRun, NormalizedItem, NormalizedPost};

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

            for (index, (file_name, item_key, position)) in fixtures.iter().enumerate() {
                if cancel.is_cancelled() {
                    return Err(RunnerFailure::terminal(
                        RunnerFailureKind::Interrupted,
                        "Guided tour subscription cancelled",
                    ));
                }
                let path = self.fixture_root.join(file_name);
                ensure_fixture(&path)?;
                let source = SourcePostInput {
                    site_id: "twitter".into(),
                    post_key: post_key.into(),
                    item_key: (*item_key).into(),
                    position: *position,
                    post_complete: index + 1 == fixtures.len(),
                    force_collection: false,
                    group_post: query.group_posts,
                    canonical_post_url: Some(format!(
                        "https://x.com/leonardo_archive/status/{post_key}"
                    )),
                    canonical_media_url: None,
                    creator_name: Some("Leonardo da Vinci Archive".into()),
                    title: Some(
                        if fixtures.len() > 1 {
                            "Lady with an Ermine studies"
                        } else {
                            "Mona Lisa"
                        }
                        .into(),
                    ),
                    description: Some("Bundled public-domain guided-tour fixture".into()),
                    captured_at: Some("1519-05-02T00:00:00Z".into()),
                    metadata_json: Some("{\"tutorial\":true,\"network\":false}".into()),
                };
                let input = crate::import_v2::prepare_input(
                    &path,
                    Lifecycle::Inbox,
                    None,
                    &["creator:leonardo da vinci".into(), "meta:tutorial".into()],
                    &[format!(
                        "https://commons.wikimedia.org/wiki/File:{file_name}"
                    )],
                    Some(source),
                )
                .await
                .map_err(|error| {
                    RunnerFailure::terminal(RunnerFailureKind::InvalidOutput, error)
                })?;
                send(
                    &output,
                    SourceEvent::MediaDownloaded(DownloadedItem {
                        post: post.clone(),
                        source_path: path,
                        input,
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
