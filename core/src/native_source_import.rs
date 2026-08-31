//! The sole native-source to canonical-library ingest assembler.

use std::path::{Path, PathBuf};

use picto_library::{ImmutableMediaFacts, Lifecycle, PreparedImport, Rating, SourceIdentity};
use picto_sources::{PostDownload, SourcePost};

use crate::media_processing::PreparedMediaSource;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedSourceMedia {
    pub media_id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedSourcePost {
    pub members: Vec<PreparedImport>,
    pub collection_name: Option<String>,
    pub cleanup_paths: Vec<PathBuf>,
    pub rejected_media: Vec<RejectedSourceMedia>,
}

struct PrepareContext<'a> {
    post: &'a SourcePost,
    source_key: &'a str,
    source_text: &'a str,
    tags: &'a [String],
    imported_at_ms: i64,
}

struct MemberKeys<'a> {
    stable: &'a str,
    source_item: &'a str,
    descriptor: &'a str,
}

pub async fn prepare_source_post(
    post: &SourcePost,
    download: PostDownload,
    imported_at_ms: i64,
) -> Result<PreparedSourcePost, String> {
    let tags = canonical_tags(post);
    let source_key = format!("{}:{}", post.site_id, post.stable_id);
    let source_text = source_search_text(post);
    let mut members = Vec::with_capacity(download.downloaded.len());
    let mut cleanup_paths = Vec::with_capacity(download.downloaded.len());
    let mut rejected_media = download
        .failures
        .into_iter()
        .map(|failure| RejectedSourceMedia {
            media_id: failure.descriptor.stable_id,
            message: failure.message,
        })
        .collect::<Vec<_>>();
    let context = PrepareContext {
        post,
        source_key: &source_key,
        source_text: &source_text,
        tags: &tags,
        imported_at_ms,
    };

    for media in download.downloaded {
        cleanup_paths.push(media.path.clone());
        match prepare_downloaded_media(&context, &media.descriptor.stable_id, &media.path).await {
            Ok(mut prepared) => members.append(&mut prepared),
            Err(message) => rejected_media.push(RejectedSourceMedia {
                media_id: media.descriptor.stable_id,
                message,
            }),
        }
    }

    Ok(PreparedSourcePost {
        collection_name: (members.len() > 1).then(|| post_title(post)),
        members,
        cleanup_paths,
        rejected_media,
    })
}

pub(crate) fn post_title(post: &SourcePost) -> String {
    post.name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{}_{}", post.site_id, post.stable_id))
}

async fn prepare_downloaded_media(
    context: &PrepareContext<'_>,
    source_item_key: &str,
    path: &Path,
) -> Result<Vec<PreparedImport>, String> {
    let inspected = PreparedMediaSource::prepare_ingest(path)
        .await
        .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
    if inspected.mime_type != "application/zip" {
        return prepare_member(
            context,
            MemberKeys {
                stable: source_item_key,
                source_item: source_item_key,
                descriptor: source_item_key,
            },
            path,
        )
        .await
        .map(|member| vec![member]);
    }

    let parent = path.parent().ok_or_else(|| {
        format!(
            "Downloaded ZIP has no staging directory: {}",
            path.display()
        )
    })?;
    let entries = crate::media_processing::archive::extract_library_files(
        path,
        &parent.join("expanded-archives"),
    )
    .map_err(|error| format!("Failed to extract {}: {error}", path.display()))?;
    if entries.is_empty() {
        return Err(format!(
            "Downloaded ZIP contains no accepted media: {}",
            path.display()
        ));
    }

    let mut members = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let member_source_key = if index == 0 {
            source_item_key.to_owned()
        } else {
            format!("{source_item_key}:zip:{index}")
        };
        let mut member = prepare_member(
            context,
            MemberKeys {
                stable: &member_source_key,
                source_item: &member_source_key,
                descriptor: source_item_key,
            },
            &entry.path,
        )
        .await?;
        member.media_name = Path::new(&entry.archive_name)
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&member.media_name)
            .to_owned();
        members.push(member);
    }
    Ok(members)
}

async fn prepare_member(
    context: &PrepareContext<'_>,
    keys: MemberKeys<'_>,
    path: &Path,
) -> Result<PreparedImport, String> {
    let prepared = PreparedMediaSource::prepare_ingest(path)
        .await
        .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
    if !prepared.caps.ingest_supported || prepared.mime_type == "application/zip" {
        return Err(format!("Unsupported source media: {}", path.display()));
    }
    let content_hash = crate::media_processing::get_hash_from_path_background(path.to_path_buf())
        .await
        .map(hex::encode)
        .map_err(|error| format!("Failed to hash {}: {error}", path.display()))?;
    let size_bytes = prepared
        .size_bytes
        .or_else(|| std::fs::metadata(path).ok().map(|metadata| metadata.len()))
        .unwrap_or_default();
    let mut source_urls = context
        .post
        .canonical_url
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    if let Some(url) = context
        .post
        .media
        .iter()
        .find(|media| media.stable_id == keys.descriptor)
        .and_then(|media| media.canonical_url.clone())
    {
        if !source_urls.contains(&url) {
            source_urls.push(url);
        }
    }
    let media_name = member_name(context.post, keys.descriptor, path);

    Ok(PreparedImport {
        stable_key: format!("source:{}:{}", context.source_key, keys.stable),
        media_name,
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
        notes: context.post.notes.clone(),
        tags: context.tags.to_vec(),
        folders: Vec::new(),
        source_urls,
        source_identity: Some(SourceIdentity {
            source_key: context.source_key.to_string(),
            source_item_key: keys.source_item.to_string(),
            source_text: (!context.source_text.is_empty()).then(|| context.source_text.to_string()),
            source_attempt_id: None,
        }),
        imported_at_ms: context.imported_at_ms,
        captured_at_ms: context
            .post
            .created_at
            .as_deref()
            .and_then(parse_source_time_ms),
    })
}

fn parse_source_time_ms(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.timestamp_millis())
        .or_else(|| {
            value
                .parse::<i64>()
                .ok()
                .and_then(|seconds| seconds.checked_mul(1_000))
        })
}

fn canonical_tags(post: &SourcePost) -> Vec<String> {
    post.tags
        .iter()
        .filter_map(|tag| {
            crate::tag_name::normalize_external_pair(&tag.namespace, &tag.value)
                .map(|(namespace, value)| crate::tag_name::format(&namespace, &value))
        })
        .collect()
}

fn member_name(post: &SourcePost, source_item_key: &str, path: &Path) -> String {
    let descriptor_name = post
        .media
        .iter()
        .find(|media| media.stable_id == source_item_key)
        .and_then(|media| media.file_name.as_deref())
        .and_then(|name| Path::new(name).file_stem())
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty());
    if post.media.len() == 1 {
        post.name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .or(descriptor_name)
    } else {
        descriptor_name
    }
    .or_else(|| path.file_stem().and_then(|name| name.to_str()))
    .unwrap_or("Untitled")
    .to_string()
}

fn source_search_text(post: &SourcePost) -> String {
    [
        post.creator.as_deref(),
        post.name.as_deref(),
        post.notes.as_deref(),
        post.canonical_url.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use picto_sources::{
        CanonicalTag, DownloadedMedia, MediaDescriptor, PostDownload, SourcePartition,
    };

    use super::*;

    fn post(media_count: usize) -> SourcePost {
        SourcePost {
            site_id: "fixture".into(),
            partition: SourcePartition::new("feed"),
            stable_id: "post-1".into(),
            canonical_url: Some("https://example.test/post/1".into()),
            creator: Some("Creator".into()),
            name: Some("Post title".into()),
            notes: None,
            created_at: None,
            tags: vec![CanonicalTag::new("meta", "Highres")],
            media: (0..media_count)
                .map(|index| MediaDescriptor {
                    stable_id: format!("media-{index}"),
                    position: index as u32,
                    url: format!("https://example.test/{index}.png"),
                    canonical_url: None,
                    file_name: Some(format!("member-{index}.png")),
                    mime_hint: Some("image/png".into()),
                    expected_size: None,
                    headers: BTreeMap::new(),
                    fallbacks: Vec::new(),
                    rejected_final_paths: Vec::new(),
                })
                .collect(),
            resume_cursor_after: None,
        }
    }

    #[test]
    fn standalone_uses_post_name_while_collection_members_keep_media_names() {
        let single = post(1);
        assert_eq!(
            member_name(&single, "media-0", Path::new("download.png")),
            "Post title",
        );
        let collection = post(2);
        assert_eq!(
            member_name(&collection, "media-1", Path::new("download.png")),
            "member-1",
        );
    }

    #[test]
    fn titleless_social_collections_receive_a_deterministic_weak_name() {
        let mut post = post(2);
        post.site_id = "twitter".into();
        post.stable_id = "2085395535410712592".into();
        post.name = None;
        assert_eq!(post_title(&post), "twitter_2085395535410712592");
    }

    #[test]
    fn source_dates_accept_rfc3339_and_unix_seconds() {
        assert_eq!(
            parse_source_time_ms("2026-08-30T12:34:56Z"),
            Some(1_788_093_296_000)
        );
        assert_eq!(parse_source_time_ms("1788093296"), Some(1_788_093_296_000));
        assert_eq!(parse_source_time_ms("not-a-date"), None);
    }

    #[test]
    fn unsupported_source_namespaces_fall_back_to_general() {
        assert_eq!(canonical_tags(&post(1)), vec!["highres"]);
    }

    #[tokio::test]
    async fn provider_zip_expands_into_canonical_collection_members() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.png");
        let second = directory.path().join("second.png");
        image::RgbImage::from_pixel(2, 2, image::Rgb([10, 20, 30]))
            .save(&first)
            .unwrap();
        image::RgbImage::from_pixel(2, 2, image::Rgb([30, 20, 10]))
            .save(&second)
            .unwrap();
        let archive = directory.path().join("attachment.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("pages/first.png", options).unwrap();
        std::io::copy(&mut std::fs::File::open(&first).unwrap(), &mut zip).unwrap();
        zip.start_file("second.png", options).unwrap();
        std::io::copy(&mut std::fs::File::open(&second).unwrap(), &mut zip).unwrap();
        zip.finish().unwrap();

        let mut source_post = post(1);
        source_post.media[0].url = "https://example.test/attachment.zip".into();
        source_post.media[0].file_name = Some("attachment.zip".into());
        source_post.media[0].mime_hint = Some("application/zip".into());
        let prepared = prepare_source_post(
            &source_post,
            PostDownload {
                downloaded: vec![DownloadedMedia {
                    descriptor: source_post.media[0].clone(),
                    path: archive.clone(),
                    size_bytes: std::fs::metadata(&archive).unwrap().len(),
                }],
                failures: Vec::new(),
            },
            1,
        )
        .await
        .unwrap();

        assert_eq!(prepared.members.len(), 2);
        assert_eq!(prepared.collection_name.as_deref(), Some("Post title"));
        assert_eq!(prepared.members[0].media_name, "first");
        assert_eq!(prepared.members[1].media_name, "second");
        assert_ne!(
            prepared.members[0].stable_key,
            prepared.members[1].stable_key
        );
        assert_eq!(
            prepared.members[0]
                .source_identity
                .as_ref()
                .map(|source| source.source_item_key.as_str()),
            Some("media-0")
        );
        assert_eq!(
            prepared.members[1]
                .source_identity
                .as_ref()
                .map(|source| source.source_item_key.as_str()),
            Some("media-0:zip:1")
        );
    }
}
