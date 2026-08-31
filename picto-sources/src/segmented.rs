use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    DownloadedMedia, HttpRuntime, MediaDelivery, MediaDescriptor, RequestCredentials, SourceError,
    SourceErrorKind,
};

const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SEGMENTS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuxInputKind {
    Video,
    Audio,
    Media,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuxInput {
    pub kind: MuxInputKind,
    pub path: PathBuf,
}

pub type MuxFuture<'a> = Pin<Box<dyn Future<Output = Result<(), SourceError>> + Send + 'a>>;

pub trait MediaMuxer: Send + Sync {
    fn mux<'a>(
        &'a self,
        inputs: &'a [MuxInput],
        destination: &'a Path,
        cancel: &'a CancellationToken,
    ) -> MuxFuture<'a>;
}

#[derive(Debug, Clone)]
pub struct FfmpegMuxer {
    executable: PathBuf,
}

impl FfmpegMuxer {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    pub fn discover() -> Self {
        let binary = if cfg!(target_os = "windows") {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        if let Ok(directory) = std::env::var("PICTO_FFMPEG_DIR") {
            let candidate = PathBuf::from(directory).join(binary);
            if candidate.is_file() {
                return Self::new(candidate);
            }
        }
        if let Ok(mut directory) = std::env::current_dir() {
            loop {
                let candidate = directory.join("vendor").join("ffmpeg").join(binary);
                if candidate.is_file() {
                    return Self::new(candidate);
                }
                if !directory.pop() {
                    break;
                }
            }
        }
        Self::new(binary)
    }
}

impl MediaMuxer for FfmpegMuxer {
    fn mux<'a>(
        &'a self,
        inputs: &'a [MuxInput],
        destination: &'a Path,
        cancel: &'a CancellationToken,
    ) -> MuxFuture<'a> {
        Box::pin(async move {
            if inputs.is_empty() {
                return Err(download_error("cannot mux media without an input", false));
            }
            let mut command = Command::new(&self.executable);
            command
                .kill_on_drop(true)
                .args(["-nostdin", "-hide_banner", "-loglevel", "error"]);
            for input in inputs {
                command.arg("-i").arg(&input.path);
            }
            command.args(["-c", "copy", "-movflags", "use_metadata_tags"]);
            for (index, input) in inputs.iter().enumerate() {
                let stream = match input.kind {
                    MuxInputKind::Video => format!("{index}:v:0"),
                    MuxInputKind::Audio => format!("{index}:a:0"),
                    MuxInputKind::Media => format!("{index}:0"),
                };
                command.arg("-map").arg(stream);
            }
            command.arg("-y").arg(destination);
            let output = tokio::select! {
                _ = cancel.cancelled() => return Err(cancelled("media mux cancelled")),
                output = command.output() => output.map_err(|error| {
                    download_error(format!("failed to run FFmpeg: {error}"), true)
                })?,
            };
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(download_error(
                    format!("FFmpeg mux failed: {}", stderr.trim()),
                    false,
                ));
            }
            Ok(())
        })
    }
}

#[derive(Clone)]
pub(crate) struct SegmentedDownloader {
    muxer: Arc<dyn MediaMuxer>,
}

impl SegmentedDownloader {
    pub(crate) fn new(muxer: Arc<dyn MediaMuxer>) -> Self {
        Self { muxer }
    }

    pub(crate) async fn download(
        &self,
        descriptor: &MediaDescriptor,
        credentials: &RequestCredentials,
        destination: &Path,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<DownloadedMedia, SourceError> {
        let downloaded = match descriptor.delivery() {
            MediaDelivery::Direct => {
                http.download(descriptor, credentials, destination, cancel)
                    .await
            }
            MediaDelivery::Dash | MediaDelivery::Hls => {
                self.download_segmented(descriptor, credentials, destination, http, cancel)
                    .await
            }
        }?;
        crate::ugoira::postprocess(downloaded, cancel).await
    }

    async fn download_segmented(
        &self,
        descriptor: &MediaDescriptor,
        credentials: &RequestCredentials,
        destination: &Path,
        http: &HttpRuntime,
        cancel: &CancellationToken,
    ) -> Result<DownloadedMedia, SourceError> {
        let manifest_url = parse_url(&descriptor.url)?;
        let work = destination.with_extension("picto-segments");
        let extension = destination
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("media");
        let temporary = work.join(format!("output.{extension}"));
        let _ = tokio::fs::remove_dir_all(&work).await;
        let _ = tokio::fs::remove_file(&temporary).await;
        tokio::fs::create_dir_all(&work)
            .await
            .map_err(|error| download_error(error.to_string(), true))?;

        let result = async {
            let manifest = fetch_manifest(
                descriptor,
                &manifest_url,
                credentials,
                &work.join("manifest"),
                http,
                cancel,
            )
            .await?;
            let plan = match descriptor.delivery() {
                MediaDelivery::Dash => parse_dash(&manifest, &manifest_url)?,
                MediaDelivery::Hls => {
                    plan_hls(
                        descriptor,
                        &manifest,
                        &manifest_url,
                        credentials,
                        &work,
                        http,
                        cancel,
                    )
                    .await?
                }
                MediaDelivery::Direct => unreachable!(),
            };
            let mut inputs = Vec::with_capacity(plan.tracks.len());
            for (track_index, track) in plan.tracks.iter().enumerate() {
                inputs.push(
                    download_track(
                        descriptor,
                        track,
                        track_index,
                        credentials,
                        &work,
                        http,
                        cancel,
                    )
                    .await?,
                );
            }
            self.muxer.mux(&inputs, &temporary, cancel).await?;
            let size_bytes = tokio::fs::metadata(&temporary)
                .await
                .map_err(|error| download_error(error.to_string(), true))?
                .len();
            if size_bytes == 0 {
                return Err(download_error(
                    "segmented media produced an empty file",
                    false,
                ));
            }
            tokio::fs::rename(&temporary, destination)
                .await
                .map_err(|error| download_error(error.to_string(), true))?;
            Ok(DownloadedMedia {
                descriptor: descriptor.clone(),
                path: destination.to_path_buf(),
                size_bytes,
            })
        }
        .await;

        if result.is_err() {
            let _ = tokio::fs::remove_file(&temporary).await;
        }
        let _ = tokio::fs::remove_dir_all(&work).await;
        result
    }
}

#[derive(Debug)]
struct MediaPlan {
    tracks: Vec<TrackPlan>,
}

#[derive(Debug)]
struct TrackPlan {
    kind: MuxInputKind,
    segments: Vec<Segment>,
}

#[derive(Debug, Clone)]
struct Segment {
    url: Url,
    range: Option<String>,
}

async fn download_track(
    descriptor: &MediaDescriptor,
    track: &TrackPlan,
    track_index: usize,
    credentials: &RequestCredentials,
    work: &Path,
    http: &HttpRuntime,
    cancel: &CancellationToken,
) -> Result<MuxInput, SourceError> {
    if track.segments.is_empty() {
        return Err(invalid_manifest("segmented track has no media segments"));
    }
    let assembled_part = work.join(format!("track-{track_index}.assembling"));
    let assembled = work.join(format!("track-{track_index}.media"));
    let mut output = tokio::fs::File::create(&assembled_part)
        .await
        .map_err(|error| download_error(error.to_string(), true))?;
    for (segment_index, segment) in track.segments.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(cancelled("segment download cancelled"));
        }
        let path = work.join(format!("track-{track_index}-segment-{segment_index}"));
        let mut segment_descriptor = MediaDescriptor {
            stable_id: format!(
                "{}:segment:{track_index}:{segment_index}",
                descriptor.stable_id
            ),
            position: segment_index as u32,
            url: segment.url.to_string(),
            canonical_url: descriptor.canonical_url.clone(),
            file_name: None,
            mime_hint: None,
            expected_size: None,
            headers: descriptor.headers.clone(),
            fallbacks: Vec::new(),
            rejected_final_paths: descriptor.rejected_final_paths.clone(),
            postprocess: None,
        };
        if let Some(range) = &segment.range {
            segment_descriptor
                .headers
                .insert("range".into(), range.clone());
        }
        http.download(&segment_descriptor, credentials, &path, cancel)
            .await?;
        let mut input = tokio::fs::File::open(&path)
            .await
            .map_err(|error| download_error(error.to_string(), true))?;
        tokio::select! {
            _ = cancel.cancelled() => return Err(cancelled("media assembly cancelled")),
            result = tokio::io::copy(&mut input, &mut output) => {
                result.map_err(|error| download_error(error.to_string(), true))?;
            }
        }
        let _ = tokio::fs::remove_file(path).await;
    }
    output
        .flush()
        .await
        .map_err(|error| download_error(error.to_string(), true))?;
    drop(output);
    tokio::fs::rename(&assembled_part, &assembled)
        .await
        .map_err(|error| download_error(error.to_string(), true))?;
    Ok(MuxInput {
        kind: track.kind,
        path: assembled,
    })
}

async fn fetch_manifest(
    descriptor: &MediaDescriptor,
    url: &Url,
    credentials: &RequestCredentials,
    path: &Path,
    http: &HttpRuntime,
    cancel: &CancellationToken,
) -> Result<String, SourceError> {
    let request = MediaDescriptor {
        stable_id: format!("{}:manifest", descriptor.stable_id),
        position: 0,
        url: url.to_string(),
        canonical_url: descriptor.canonical_url.clone(),
        file_name: None,
        mime_hint: None,
        expected_size: None,
        headers: descriptor.headers.clone(),
        fallbacks: Vec::new(),
        rejected_final_paths: descriptor.rejected_final_paths.clone(),
        postprocess: None,
    };
    let downloaded = http.download(&request, credentials, path, cancel).await?;
    if downloaded.size_bytes > MAX_MANIFEST_BYTES {
        return Err(invalid_manifest("media manifest exceeds 8 MiB"));
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| download_error(error.to_string(), true))?;
    let _ = tokio::fs::remove_file(path).await;
    String::from_utf8(bytes).map_err(|_| invalid_manifest("media manifest is not UTF-8"))
}

fn parse_dash(manifest: &str, manifest_url: &Url) -> Result<MediaPlan, SourceError> {
    let document = roxmltree::Document::parse(manifest)
        .map_err(|error| invalid_manifest(format!("invalid DASH manifest: {error}")))?;
    let mpd = document.root_element();
    if mpd.tag_name().name() != "MPD" {
        return Err(invalid_manifest("DASH document has no MPD root"));
    }
    if mpd.attribute("type") == Some("dynamic") {
        return Err(invalid_manifest("live DASH manifests are unsupported"));
    }
    if document
        .descendants()
        .any(|node| node.is_element() && node.tag_name().name() == "ContentProtection")
    {
        return Err(protected_manifest("DASH ContentProtection"));
    }
    let periods = children(mpd, "Period").collect::<Vec<_>>();
    if periods.len() != 1 {
        return Err(invalid_manifest(
            "DASH media must contain exactly one Period",
        ));
    }
    let period = periods[0];
    let duration = period
        .attribute("duration")
        .or_else(|| mpd.attribute("mediaPresentationDuration"))
        .and_then(parse_iso_duration);
    let root_base = node_base(manifest_url, mpd)?;
    let period_base = node_base(&root_base, period)?;
    let mut best_video = None;
    let mut best_audio = None;
    for adaptation in children(period, "AdaptationSet") {
        let adaptation_base = node_base(&period_base, adaptation)?;
        let mut representations = children(adaptation, "Representation")
            .filter_map(|representation| {
                dash_kind(adaptation, representation).map(|kind| (kind, representation))
            })
            .collect::<Vec<_>>();
        representations.sort_by_key(|(_, representation)| {
            representation
                .attribute("bandwidth")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0)
        });
        let Some((kind, representation)) = representations.pop() else {
            continue;
        };
        let bandwidth = representation
            .attribute("bandwidth")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let representation_base = node_base(&adaptation_base, representation)?;
        let segments = dash_segments(representation, adaptation, &representation_base, duration)?;
        let candidate = (bandwidth, TrackPlan { kind, segments });
        let selected = match kind {
            MuxInputKind::Video => &mut best_video,
            MuxInputKind::Audio => &mut best_audio,
            MuxInputKind::Media => continue,
        };
        if selected
            .as_ref()
            .is_none_or(|(selected_bandwidth, _)| bandwidth > *selected_bandwidth)
        {
            *selected = Some(candidate);
        }
    }
    let tracks = best_video
        .into_iter()
        .chain(best_audio)
        .map(|(_, track)| track)
        .collect::<Vec<_>>();
    if tracks.is_empty() {
        return Err(invalid_manifest(
            "DASH manifest has no clear audio or video",
        ));
    }
    Ok(MediaPlan { tracks })
}

fn dash_kind(
    adaptation: roxmltree::Node<'_, '_>,
    representation: roxmltree::Node<'_, '_>,
) -> Option<MuxInputKind> {
    let content_type = representation
        .attribute("contentType")
        .or_else(|| adaptation.attribute("contentType"));
    let mime = representation
        .attribute("mimeType")
        .or_else(|| adaptation.attribute("mimeType"));
    match (content_type, mime) {
        (Some("video"), _) | (_, Some("video/mp4")) => Some(MuxInputKind::Video),
        (Some("audio"), _) | (_, Some("audio/mp4")) => Some(MuxInputKind::Audio),
        _ => None,
    }
}

fn dash_segments(
    representation: roxmltree::Node<'_, '_>,
    adaptation: roxmltree::Node<'_, '_>,
    base: &Url,
    duration_seconds: Option<f64>,
) -> Result<Vec<Segment>, SourceError> {
    if let Some(list) =
        child(representation, "SegmentList").or_else(|| child(adaptation, "SegmentList"))
    {
        let mut segments = Vec::new();
        if let Some(initialization) = child(list, "Initialization") {
            if let Some(source) = initialization.attribute("sourceURL") {
                segments.push(Segment {
                    url: resolve_url(base, source)?,
                    range: byte_range(initialization.attribute("range"))?,
                });
            }
        }
        for segment in children(list, "SegmentURL") {
            let media = segment
                .attribute("media")
                .ok_or_else(|| invalid_manifest("DASH SegmentURL has no media URL"))?;
            segments.push(Segment {
                url: resolve_url(base, media)?,
                range: byte_range(segment.attribute("mediaRange"))?,
            });
        }
        ensure_segment_limit(&segments)?;
        return Ok(segments);
    }
    if let Some(template) =
        child(representation, "SegmentTemplate").or_else(|| child(adaptation, "SegmentTemplate"))
    {
        return dash_template_segments(template, representation, base, duration_seconds);
    }
    Ok(vec![Segment {
        url: base.clone(),
        range: None,
    }])
}

fn dash_template_segments(
    template: roxmltree::Node<'_, '_>,
    representation: roxmltree::Node<'_, '_>,
    base: &Url,
    duration_seconds: Option<f64>,
) -> Result<Vec<Segment>, SourceError> {
    let representation_id = representation.attribute("id").unwrap_or("");
    let bandwidth = representation.attribute("bandwidth").unwrap_or("");
    let mut segments = Vec::new();
    if let Some(initialization) = template.attribute("initialization") {
        let path = expand_template(initialization, representation_id, bandwidth, None, None)?;
        segments.push(Segment {
            url: resolve_url(base, &path)?,
            range: None,
        });
    }
    let media = template
        .attribute("media")
        .ok_or_else(|| invalid_manifest("DASH SegmentTemplate has no media pattern"))?;
    let timescale = parse_u64(template.attribute("timescale").unwrap_or("1"), "timescale")?;
    if timescale == 0 {
        return Err(invalid_manifest("DASH timescale must be greater than zero"));
    }
    let start_number = parse_u64(
        template.attribute("startNumber").unwrap_or("1"),
        "startNumber",
    )?;
    if let Some(timeline) = child(template, "SegmentTimeline") {
        let mut current_time = 0_u64;
        let mut number = start_number;
        for entry in children(timeline, "S") {
            if let Some(value) = entry.attribute("t") {
                current_time = parse_u64(value, "timeline time")?;
            }
            let duration = parse_u64(
                entry
                    .attribute("d")
                    .ok_or_else(|| invalid_manifest("DASH timeline entry has no duration"))?,
                "timeline duration",
            )?;
            let repeat = entry
                .attribute("r")
                .unwrap_or("0")
                .parse::<i64>()
                .map_err(|_| invalid_manifest("invalid DASH timeline repeat"))?;
            if repeat < 0 {
                return Err(invalid_manifest(
                    "open-ended DASH timeline repeats are unsupported",
                ));
            }
            for _ in 0..=repeat {
                let path = expand_template(
                    media,
                    representation_id,
                    bandwidth,
                    Some(number),
                    Some(current_time),
                )?;
                segments.push(Segment {
                    url: resolve_url(base, &path)?,
                    range: None,
                });
                current_time = current_time.saturating_add(duration);
                number = number.saturating_add(1);
                ensure_segment_limit(&segments)?;
            }
        }
    } else {
        let segment_duration = parse_u64(
            template
                .attribute("duration")
                .ok_or_else(|| invalid_manifest("DASH template has no finite timeline"))?,
            "segment duration",
        )?;
        let total = duration_seconds
            .ok_or_else(|| invalid_manifest("DASH template has no finite media duration"))?;
        let count = ((total * timescale as f64) / segment_duration as f64).ceil() as usize;
        if count == 0 || count > MAX_SEGMENTS {
            return Err(invalid_manifest("invalid DASH segment count"));
        }
        for offset in 0..count {
            let number = start_number.saturating_add(offset as u64);
            let path = expand_template(media, representation_id, bandwidth, Some(number), None)?;
            segments.push(Segment {
                url: resolve_url(base, &path)?,
                range: None,
            });
        }
    }
    if segments.is_empty() {
        return Err(invalid_manifest("DASH template produced no segments"));
    }
    Ok(segments)
}

async fn plan_hls(
    descriptor: &MediaDescriptor,
    manifest: &str,
    manifest_url: &Url,
    credentials: &RequestCredentials,
    work: &Path,
    http: &HttpRuntime,
    cancel: &CancellationToken,
) -> Result<MediaPlan, SourceError> {
    if manifest.lines().any(|line| {
        line.starts_with("#EXT-X-SESSION-KEY")
            || (line.starts_with("#EXT-X-KEY") && !line.contains("METHOD=NONE"))
    }) {
        return Err(protected_manifest("HLS encryption key"));
    }
    let variants = hls_variants(manifest, manifest_url)?;
    if variants.is_empty() {
        return Ok(MediaPlan {
            tracks: vec![parse_hls_media(
                manifest,
                manifest_url,
                MuxInputKind::Media,
            )?],
        });
    }
    let selected = variants
        .into_iter()
        .max_by_key(|variant| variant.bandwidth)
        .expect("variants is not empty");
    let video_manifest = fetch_manifest(
        descriptor,
        &selected.url,
        credentials,
        &work.join("video-playlist"),
        http,
        cancel,
    )
    .await?;
    let mut tracks = vec![parse_hls_media(
        &video_manifest,
        &selected.url,
        MuxInputKind::Video,
    )?];
    if let Some(group) = selected.audio_group {
        if let Some(audio_url) = hls_audio_rendition(manifest, manifest_url, &group)? {
            let audio_manifest = fetch_manifest(
                descriptor,
                &audio_url,
                credentials,
                &work.join("audio-playlist"),
                http,
                cancel,
            )
            .await?;
            tracks.push(parse_hls_media(
                &audio_manifest,
                &audio_url,
                MuxInputKind::Audio,
            )?);
        }
    }
    Ok(MediaPlan { tracks })
}

#[derive(Debug)]
struct HlsVariant {
    bandwidth: u64,
    audio_group: Option<String>,
    url: Url,
}

fn hls_variants(manifest: &str, base: &Url) -> Result<Vec<HlsVariant>, SourceError> {
    let lines = manifest.lines().map(str::trim).collect::<Vec<_>>();
    let mut variants = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(attributes) = line.strip_prefix("#EXT-X-STREAM-INF:") else {
            continue;
        };
        let uri = lines[index + 1..]
            .iter()
            .find(|candidate| !candidate.is_empty() && !candidate.starts_with('#'))
            .ok_or_else(|| invalid_manifest("HLS variant has no playlist URI"))?;
        let attributes = hls_attributes(attributes)?;
        let bandwidth = attributes
            .get("BANDWIDTH")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        variants.push(HlsVariant {
            bandwidth,
            audio_group: attributes.get("AUDIO").cloned(),
            url: resolve_url(base, uri)?,
        });
    }
    Ok(variants)
}

fn hls_audio_rendition(
    manifest: &str,
    base: &Url,
    group: &str,
) -> Result<Option<Url>, SourceError> {
    let mut fallback = None;
    for line in manifest.lines().map(str::trim) {
        let Some(raw) = line.strip_prefix("#EXT-X-MEDIA:") else {
            continue;
        };
        let attributes = hls_attributes(raw)?;
        if attributes.get("TYPE").map(String::as_str) != Some("AUDIO")
            || attributes.get("GROUP-ID").map(String::as_str) != Some(group)
        {
            continue;
        }
        let Some(uri) = attributes.get("URI") else {
            continue;
        };
        let resolved = resolve_url(base, uri)?;
        if attributes.get("DEFAULT").map(String::as_str) == Some("YES") {
            return Ok(Some(resolved));
        }
        fallback.get_or_insert(resolved);
    }
    Ok(fallback)
}

fn parse_hls_media(
    manifest: &str,
    base: &Url,
    kind: MuxInputKind,
) -> Result<TrackPlan, SourceError> {
    if !manifest.lines().any(|line| line.trim() == "#EXT-X-ENDLIST") {
        return Err(invalid_manifest("live HLS playlists are unsupported"));
    }
    let mut segments = Vec::new();
    let mut pending_range = None;
    let mut next_offset = 0_u64;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with("#EXT-X-SESSION-KEY")
            || (line.starts_with("#EXT-X-KEY") && !line.contains("METHOD=NONE"))
        {
            return Err(protected_manifest("HLS encryption key"));
        }
        if let Some(raw) = line.strip_prefix("#EXT-X-MAP:") {
            let attributes = hls_attributes(raw)?;
            let uri = attributes
                .get("URI")
                .ok_or_else(|| invalid_manifest("HLS map has no URI"))?;
            segments.push(Segment {
                url: resolve_url(base, uri)?,
                range: attributes
                    .get("BYTERANGE")
                    .map(|value| hls_byte_range(value, &mut next_offset))
                    .transpose()?,
            });
        } else if let Some(raw) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            pending_range = Some(hls_byte_range(raw, &mut next_offset)?);
        } else if !line.is_empty() && !line.starts_with('#') {
            segments.push(Segment {
                url: resolve_url(base, line)?,
                range: pending_range.take(),
            });
            ensure_segment_limit(&segments)?;
        }
    }
    if segments.is_empty() {
        return Err(invalid_manifest("HLS playlist has no media segments"));
    }
    Ok(TrackPlan { kind, segments })
}

fn hls_attributes(value: &str) -> Result<BTreeMap<String, String>, SourceError> {
    let mut attributes = BTreeMap::new();
    let mut start = 0;
    let mut quoted = false;
    let bytes = value.as_bytes();
    for index in 0..=bytes.len() {
        if index < bytes.len() && bytes[index] == b'"' {
            quoted = !quoted;
        }
        if index == bytes.len() || (bytes[index] == b',' && !quoted) {
            let entry = value[start..index].trim();
            if let Some((key, raw)) = entry.split_once('=') {
                attributes.insert(key.to_string(), raw.trim_matches('"').to_string());
            }
            start = index.saturating_add(1);
        }
    }
    if quoted {
        return Err(invalid_manifest("unterminated HLS attribute quote"));
    }
    Ok(attributes)
}

fn hls_byte_range(value: &str, next_offset: &mut u64) -> Result<String, SourceError> {
    let (length, explicit_offset) = value
        .trim_matches('"')
        .split_once('@')
        .map_or((value.trim_matches('"'), None), |(length, offset)| {
            (length, Some(offset))
        });
    let length = parse_u64(length, "HLS byte-range length")?;
    if length == 0 {
        return Err(invalid_manifest("HLS byte range must not be empty"));
    }
    let start = explicit_offset
        .map(|offset| parse_u64(offset, "HLS byte-range offset"))
        .transpose()?
        .unwrap_or(*next_offset);
    let end = start
        .checked_add(length - 1)
        .ok_or_else(|| invalid_manifest("HLS byte range overflow"))?;
    *next_offset = end.saturating_add(1);
    Ok(format!("bytes={start}-{end}"))
}

fn expand_template(
    template: &str,
    representation_id: &str,
    bandwidth: &str,
    number: Option<u64>,
    time: Option<u64>,
) -> Result<String, SourceError> {
    let escaped = template.replace("$$", "\0");
    let mut value = escaped
        .replace("$RepresentationID$", representation_id)
        .replace("$Bandwidth$", bandwidth);
    if let Some(number) = number {
        value = value.replace("$Number$", &number.to_string());
    }
    if let Some(time) = time {
        value = value.replace("$Time$", &time.to_string());
    }
    value = value.replace('\0', "$");
    if value.contains('$') {
        return Err(invalid_manifest(format!(
            "unsupported DASH template expression in {template}"
        )));
    }
    Ok(value)
}

fn node_base(base: &Url, node: roxmltree::Node<'_, '_>) -> Result<Url, SourceError> {
    match child(node, "BaseURL")
        .and_then(|base| base.text())
        .map(str::trim)
    {
        Some(value) if !value.is_empty() => resolve_url(base, value),
        _ => Ok(base.clone()),
    }
}

fn resolve_url(base: &Url, value: &str) -> Result<Url, SourceError> {
    let mut resolved = base
        .join(value)
        .map_err(|error| invalid_manifest(format!("invalid segment URL: {error}")))?;
    if resolved.query().is_none() {
        resolved.set_query(base.query());
    }
    Ok(resolved)
}

fn parse_url(value: &str) -> Result<Url, SourceError> {
    Url::parse(value).map_err(|error| invalid_manifest(format!("invalid media URL: {error}")))
}

fn child<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    name: &'static str,
) -> Option<roxmltree::Node<'a, 'input>> {
    children(node, name).next()
}

fn children<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    name: &'static str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'input>> {
    node.children()
        .filter(move |child| child.is_element() && child.tag_name().name() == name)
}

fn byte_range(value: Option<&str>) -> Result<Option<String>, SourceError> {
    value
        .map(|value| {
            let (start, end) = value
                .split_once('-')
                .ok_or_else(|| invalid_manifest("invalid DASH byte range"))?;
            parse_u64(start, "DASH byte-range start")?;
            parse_u64(end, "DASH byte-range end")?;
            Ok(format!("bytes={start}-{end}"))
        })
        .transpose()
}

fn parse_u64(value: &str, field: &str) -> Result<u64, SourceError> {
    value
        .parse::<u64>()
        .map_err(|_| invalid_manifest(format!("invalid {field}")))
}

fn parse_iso_duration(value: &str) -> Option<f64> {
    let value = value.strip_prefix("PT")?;
    let mut total = 0.0;
    let mut start = 0;
    for (index, character) in value.char_indices() {
        let multiplier = match character {
            'H' => 3600.0,
            'M' => 60.0,
            'S' => 1.0,
            _ => continue,
        };
        total += value[start..index].parse::<f64>().ok()? * multiplier;
        start = index + character.len_utf8();
    }
    (start == value.len() && total.is_finite()).then_some(total)
}

fn ensure_segment_limit(segments: &[Segment]) -> Result<(), SourceError> {
    if segments.len() > MAX_SEGMENTS {
        Err(invalid_manifest("media manifest has too many segments"))
    } else {
        Ok(())
    }
}

fn protected_manifest(kind: &str) -> SourceError {
    SourceError::new(
        SourceErrorKind::Download,
        format!(
            "protected media is unsupported: {kind} requires a content key from a provisioned CDM/device or an external license/key service"
        ),
        false,
    )
}

fn invalid_manifest(message: impl Into<String>) -> SourceError {
    SourceError::new(SourceErrorKind::InvalidResponse, message, false)
}

fn download_error(message: impl Into<String>, retryable: bool) -> SourceError {
    SourceError::new(SourceErrorKind::Download, message, retryable)
}

fn cancelled(message: &str) -> SourceError {
    SourceError::new(SourceErrorKind::Cancelled, message, true)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::{HttpPolicy, PostDownloader, SourcePartition, SourcePost};

    struct ConcatenateMuxer;

    impl MediaMuxer for ConcatenateMuxer {
        fn mux<'a>(
            &'a self,
            inputs: &'a [MuxInput],
            destination: &'a Path,
            cancel: &'a CancellationToken,
        ) -> MuxFuture<'a> {
            Box::pin(async move {
                let mut output = tokio::fs::File::create(destination)
                    .await
                    .map_err(|error| download_error(error.to_string(), true))?;
                for input in inputs {
                    let mut file = tokio::fs::File::open(&input.path)
                        .await
                        .map_err(|error| download_error(error.to_string(), true))?;
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(cancelled("test mux cancelled")),
                        result = tokio::io::copy(&mut file, &mut output) => {
                            result.map_err(|error| download_error(error.to_string(), true))?;
                        }
                    }
                }
                output
                    .flush()
                    .await
                    .map_err(|error| download_error(error.to_string(), true))
            })
        }
    }

    #[test]
    fn dash_selects_highest_video_and_preserves_signed_query() {
        let manifest = include_str!("../tests/fixtures/segmented/clear.mpd");
        let base = Url::parse("https://cdn.example/media/manifest.mpd?Policy=signed").unwrap();
        let plan = parse_dash(manifest, &base).unwrap();
        assert_eq!(plan.tracks.len(), 2);
        assert_eq!(plan.tracks[0].kind, MuxInputKind::Video);
        assert_eq!(plan.tracks[0].segments.len(), 3);
        assert_eq!(
            plan.tracks[0].segments[1].url.as_str(),
            "https://cdn.example/media/video/high-10.m4s?Policy=signed"
        );
        assert_eq!(plan.tracks[1].kind, MuxInputKind::Audio);
    }

    #[test]
    fn hls_master_selects_highest_variant_and_audio_group() {
        let manifest = include_str!("../tests/fixtures/segmented/master.m3u8");
        let base = Url::parse("https://cdn.example/root/master.m3u8?token=signed").unwrap();
        let variants = hls_variants(manifest, &base).unwrap();
        let selected = variants
            .into_iter()
            .max_by_key(|variant| variant.bandwidth)
            .unwrap();
        assert_eq!(selected.audio_group.as_deref(), Some("main"));
        assert_eq!(
            selected.url.as_str(),
            "https://cdn.example/root/high/video.m3u8?token=signed"
        );
        assert_eq!(
            hls_audio_rendition(manifest, &base, "main")
                .unwrap()
                .unwrap()
                .as_str(),
            "https://cdn.example/root/audio/en.m3u8?token=signed"
        );
    }

    #[test]
    fn protected_manifests_fail_at_the_real_key_boundary() {
        let base = Url::parse("https://cdn.example/protected.mpd").unwrap();
        let error = parse_dash(
            r#"<MPD><Period><AdaptationSet><ContentProtection schemeIdUri="urn:mpeg:dash:mp4protection:2011"/></AdaptationSet></Period></MPD>"#,
            &base,
        )
        .unwrap_err();
        assert_eq!(error.kind, SourceErrorKind::Download);
        assert!(error.message.contains("provisioned CDM/device"));
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn hls_segments_use_runtime_retries_and_publish_one_atomic_file() {
        let (base_url, server) = fixture_server();
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("000000.ts");
        let descriptor = MediaDescriptor {
            stable_id: "media-1".into(),
            position: 0,
            url: format!("{base_url}/playlist.m3u8?token=signed"),
            canonical_url: None,
            file_name: Some("video.ts".into()),
            mime_hint: Some("application/vnd.apple.mpegurl".into()),
            expected_size: None,
            headers: BTreeMap::from([("x-media-auth".into(), "allowed".into())]),
            fallbacks: Vec::new(),
            rejected_final_paths: Vec::new(),
            postprocess: None,
        };
        let runtime = HttpRuntime::new(HttpPolicy {
            maximum_concurrency: 1,
            minimum_interval: Duration::ZERO,
            maximum_interval: Duration::ZERO,
            request_timeout: Duration::from_secs(2),
            retries: 1,
        })
        .unwrap();
        let downloader = PostDownloader::new(1)
            .unwrap()
            .with_muxer(Arc::new(ConcatenateMuxer));
        let post = SourcePost {
            site_id: "fixture".into(),
            partition: SourcePartition::new("feed"),
            stable_id: "post-1".into(),
            canonical_url: None,
            creator: None,
            name: None,
            notes: None,
            created_at: None,
            tags: Vec::new(),
            media: vec![descriptor],
            resume_cursor_after: None,
        };
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();

        assert!(!destination.exists());
        let result = downloader
            .download(
                &post,
                &RequestCredentials::default(),
                directory.path(),
                &runtime,
                &CancellationToken::new(),
                Some(&progress_tx),
            )
            .await
            .unwrap();
        let downloaded = &result.downloaded[0];

        assert_eq!(tokio::fs::read(&destination).await.unwrap(), b"INITONETWO");
        assert_eq!(downloaded.path, destination);
        assert_eq!(downloaded.size_bytes, 10);
        assert!(result.failures.is_empty());
        let progress = progress_rx.recv().await.unwrap();
        assert_eq!(progress.post_id, "post-1");
        assert_eq!(progress.media_id, "media-1");
        assert_eq!((progress.completed, progress.total), (1, 1));
        assert!(progress.succeeded);
        assert!(progress_rx.try_recv().is_err());
        assert!(!directory.path().join("000000.picto-segments").exists());
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 5);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.starts_with("GET /one.ts"))
                .count(),
            2
        );
        assert!(requests
            .iter()
            .all(|request| request.contains("token=signed")));
        assert!(requests.iter().all(|request| request
            .to_ascii_lowercase()
            .contains("x-media-auth: allowed")));
    }

    fn fixture_server() -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut requests = Vec::new();
            let mut one_attempts = 0;
            while requests.len() < 5 && Instant::now() < deadline {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut buffer = [0_u8; 8192];
                let length = stream.read(&mut buffer).unwrap();
                let request = String::from_utf8_lossy(&buffer[..length]).to_string();
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");
                let (status, body) = if path.starts_with("/playlist.m3u8") {
                    (
                        "200 OK",
                        "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:2,\none.ts\n#EXTINF:2,\ntwo.ts\n#EXT-X-ENDLIST\n",
                    )
                } else if path.starts_with("/init.mp4") {
                    ("200 OK", "INIT")
                } else if path.starts_with("/one.ts") {
                    one_attempts += 1;
                    if one_attempts == 1 {
                        ("500 Internal Server Error", "retry")
                    } else {
                        ("200 OK", "ONE")
                    }
                } else if path.starts_with("/two.ts") {
                    ("200 OK", "TWO")
                } else {
                    ("404 Not Found", "missing")
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                requests.push(request);
            }
            requests
        });
        (format!("http://{address}"), handle)
    }
}
