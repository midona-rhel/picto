use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::{stream, stream::BoxStream, StreamExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::segmented::{FfmpegMuxer, MediaMuxer, SegmentedDownloader};
use crate::{
    DownloadedMedia, HttpRuntime, MediaDelivery, MediaDescriptor, RequestCredentials, SourceError,
    SourceErrorKind, SourcePost,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaDownloadFailure {
    pub descriptor: MediaDescriptor,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostDownload {
    pub downloaded: Vec<DownloadedMedia>,
    pub failures: Vec<MediaDownloadFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadProgress {
    pub post_id: String,
    pub media_id: String,
    pub completed: usize,
    pub total: usize,
    pub succeeded: bool,
}

#[derive(Clone)]
pub struct PostDownloader {
    maximum_concurrency: usize,
    segmented: SegmentedDownloader,
}

impl std::fmt::Debug for PostDownloader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PostDownloader")
            .field("maximum_concurrency", &self.maximum_concurrency)
            .finish_non_exhaustive()
    }
}

impl PostDownloader {
    pub fn new(maximum_concurrency: usize) -> Result<Self, SourceError> {
        if maximum_concurrency == 0 {
            return Err(SourceError::new(
                SourceErrorKind::InvalidQuery,
                "post download concurrency must be greater than zero",
                false,
            ));
        }
        Ok(Self {
            maximum_concurrency,
            segmented: SegmentedDownloader::new(Arc::new(FfmpegMuxer::discover())),
        })
    }

    pub fn with_muxer(mut self, muxer: Arc<dyn MediaMuxer>) -> Self {
        self.segmented = SegmentedDownloader::new(muxer);
        self
    }

    pub async fn download(
        &self,
        post: &SourcePost,
        credentials: &RequestCredentials,
        staging_directory: &Path,
        http: &HttpRuntime,
        cancel: &CancellationToken,
        progress: Option<&mpsc::UnboundedSender<DownloadProgress>>,
    ) -> Result<PostDownload, SourceError> {
        let total = post.media.len();
        let post_id = post.stable_id.clone();
        let mut results = self
            .stream(post, credentials, staging_directory, http, cancel)
            .await?;

        let mut downloaded = Vec::with_capacity(total);
        let mut failures = Vec::new();
        let mut completed = 0;
        while let Some((descriptor, result)) = results.next().await {
            completed += 1;
            let succeeded = result.is_ok();
            match result {
                Ok(media) => downloaded.push(media),
                Err(error) if error.kind == SourceErrorKind::Cancelled => return Err(error),
                Err(error) => failures.push(MediaDownloadFailure {
                    descriptor: descriptor.clone(),
                    message: error.message,
                    retryable: error.retryable,
                }),
            }
            if let Some(progress) = progress {
                let _ = progress.send(DownloadProgress {
                    post_id: post_id.clone(),
                    media_id: descriptor.stable_id,
                    completed,
                    total,
                    succeeded,
                });
            }
        }

        downloaded.sort_by_key(|media| media.descriptor.position);
        failures.sort_by_key(|failure| failure.descriptor.position);
        Ok(PostDownload {
            downloaded,
            failures,
        })
    }

    pub async fn stream<'a>(
        &'a self,
        post: &'a SourcePost,
        credentials: &'a RequestCredentials,
        staging_directory: &'a Path,
        http: &'a HttpRuntime,
        cancel: &'a CancellationToken,
    ) -> Result<DownloadStream<'a>, SourceError> {
        tokio::fs::create_dir_all(staging_directory)
            .await
            .map_err(|error| {
                SourceError::new(SourceErrorKind::Download, error.to_string(), true)
            })?;
        Ok(Box::pin(
            stream::iter(post.media.iter().cloned().map(move |descriptor| {
                let destination = destination_path(staging_directory, &descriptor);
                async move {
                    let result = self
                        .segmented
                        .download(&descriptor, credentials, &destination, http, cancel)
                        .await;
                    (descriptor, result)
                }
            }))
            .buffer_unordered(self.maximum_concurrency),
        ))
    }
}

pub type DownloadStream<'a> =
    BoxStream<'a, (MediaDescriptor, Result<DownloadedMedia, SourceError>)>;

fn destination_path(directory: &Path, descriptor: &MediaDescriptor) -> PathBuf {
    let extension = descriptor
        .file_name
        .as_deref()
        .and_then(|name| Path::new(name).extension())
        .and_then(|extension| extension.to_str())
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= 16
                && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        .or_else(|| (descriptor.delivery() != MediaDelivery::Direct).then_some("mp4"));
    let name = match extension {
        Some(extension) => format!("{:06}.{extension}", descriptor.position),
        None => format!("{:06}.media", descriptor.position),
    };
    directory.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_paths_ignore_untrusted_source_names() {
        let directory = Path::new("/tmp/attempt");
        let descriptor = MediaDescriptor {
            stable_id: "post:media".into(),
            position: 7,
            url: "https://example.test/media".into(),
            canonical_url: None,
            file_name: Some("../../unsafe.JPG".into()),
            mime_hint: None,
            expected_size: None,
            headers: Default::default(),
        };
        assert_eq!(
            destination_path(directory, &descriptor),
            directory.join("000007.JPG")
        );
    }
}
