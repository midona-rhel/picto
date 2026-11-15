//! Production's release feed, read as `updateService.mjs` and
//! electron-updater read it: GitHub's list of releases, the newest
//! application release newer than this version, and that release's
//! `latest.yml` (Windows) or `latest-linux.yml` (Linux), which names the
//! installer, its SHA-512 and its size.
use base64::Engine as _;
use serde_json::Value;
use sha2::{Digest as _, Sha512};

/// `RELEASES_API`
pub const RELEASES_API: &str =
    "https://api.github.com/repos/midona-rhel/picto/releases?per_page=20";
/// `RELEASES_PAGE`
pub const RELEASES_PAGE: &str = "https://github.com/midona-rhel/picto/releases";
/// `RELEASE_DOWNLOADS`
pub const RELEASE_DOWNLOADS: &str = "https://github.com/midona-rhel/picto/releases/download";

/// A release from GitHub's list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Release {
    pub tag: String,
    pub name: Option<String>,
    pub body: String,
    pub published_at: Option<String>,
    pub html_url: Option<String>,
}

/// `latestAppRelease`: the first release in GitHub's order (newest first)
/// that is an application release (`APP_RELEASE_TAG`, not a draft) newer
/// than `current`.
pub fn latest_app_release(releases: &Value, current: &str) -> Option<Release> {
    releases.as_array()?.iter().find_map(|entry| {
        let tag = entry.get("tag_name")?.as_str()?;
        let draft = entry.get("draft").and_then(Value::as_bool).unwrap_or(false);
        if draft || !is_app_tag(tag) || !is_newer(tag, current) {
            return None;
        }
        let text = |key: &str| {
            entry
                .get(key)
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        Some(Release {
            tag: tag.to_owned(),
            name: text("name"),
            body: text("body").unwrap_or_default(),
            published_at: text("published_at"),
            html_url: text("html_url"),
        })
    })
}

/// `APP_RELEASE_TAG`: `v1.2.3`, optionally with a pre-release and build
/// part (`v0.6.1-alpha`), and nothing else (not `ai-models-v2`).
fn is_app_tag(tag: &str) -> bool {
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    let (rest, build) = match tag.split_once('+') {
        Some((rest, build)) => (rest, Some(build)),
        None => (tag, None),
    };
    let (core, pre) = match rest.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (rest, None),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    let identifiers = |text: &str| {
        text.split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphanumeric()))
    };
    numbers.len() == 3
        && numbers
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && pre.is_none_or(identifiers)
        && build.is_none_or(identifiers)
}

#[derive(Clone, Debug, PartialEq)]
enum Part {
    Number(u64),
    Text(String),
}

/// `versionParts`
fn parts(version: &str) -> Vec<Part> {
    version
        .strip_prefix('v')
        .unwrap_or(version)
        .split(['.', '-'])
        .map(|part| match part.parse::<u64>() {
            Ok(number) if part.bytes().all(|byte| byte.is_ascii_digit()) => Part::Number(number),
            _ => Part::Text(part.to_owned()),
        })
        .collect()
}

/// `isNewer`: compared part by part; a missing part counts as 0, a number
/// is newer than a word (`0.6.1` after `0.6.1-alpha`), words compare as
/// text.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (left, right) = (parts(candidate), parts(current));
    for index in 0..left.len().max(right.len()) {
        let a = left.get(index).cloned().unwrap_or(Part::Number(0));
        let b = right.get(index).cloned().unwrap_or(Part::Number(0));
        if a == b {
            continue;
        }
        return match (a, b) {
            (Part::Number(a), Part::Number(b)) => a > b,
            (Part::Number(_), Part::Text(_)) => true,
            (Part::Text(_), Part::Number(_)) => false,
            (Part::Text(a), Part::Text(b)) => a > b,
        };
    }
    false
}

/// The feed file electron-updater reads on each system, beside the
/// release's assets; macOS has none (production opens the release page).
pub fn feed_file(platform: &str) -> Option<&'static str> {
    match platform {
        "win32" => Some("latest.yml"),
        "linux" => Some("latest-linux.yml"),
        _ => None,
    }
}

/// The address of `file` in the release tagged `tag` (the generic feed's
/// `url`, `<downloads>/<tag>/`).
pub fn asset_url(tag: &str, file: &str) -> String {
    let base = format!(
        "{RELEASE_DOWNLOADS}/{}/",
        url::form_urlencoded::byte_serialize(tag.as_bytes()).collect::<String>()
    );
    url::Url::parse(&base)
        .and_then(|base| base.join(file))
        .map(|url| url.to_string())
        .unwrap_or_else(|_| format!("{base}{file}"))
}

/// What a feed file says of the update.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feed {
    pub version: String,
    pub release_name: Option<String>,
    pub release_date: Option<String>,
    pub release_notes: Option<String>,
    pub files: Vec<FeedFile>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeedFile {
    pub url: String,
    pub sha512: String,
    pub size: Option<u64>,
    /// `isAdminRightsRequired`: the Windows installer is started elevated
    /// (`NsisUpdater.doInstall`).
    pub admin: bool,
}

impl FeedFile {
    /// The installer's file name: the last part of its `url`, which must be
    /// a plain name.
    pub fn name(&self) -> Option<String> {
        self.url
            .split(['?', '#'])
            .next()
            .and_then(|path| path.rsplit('/').next())
            .map(str::to_owned)
            .filter(|name| !name.is_empty() && !name.contains(['/', '\\']) && name != "..")
    }
}

/// Reads the YAML electron-builder writes: top-level `key: value` lines,
/// the `files:` list, block-scalar release notes, and the older `path` and
/// `sha512` of a feed without `files`.
pub fn parse_feed(text: &str) -> Result<Feed, String> {
    let mut feed = Feed::default();
    let (mut path, mut sha512) = (None, None);
    let lines: Vec<&str> = text.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        if line.trim().is_empty() || line.trim_start().starts_with('#') || line.starts_with(' ') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "version" => feed.version = scalar(value),
            "releaseName" => feed.release_name = Some(scalar(value)),
            "releaseDate" => feed.release_date = Some(scalar(value)),
            "path" => path = Some(scalar(value)),
            "sha512" => sha512 = Some(scalar(value)),
            "releaseNotes" => {
                if value.starts_with('|') || value.starts_with('>') {
                    let mut block = Vec::new();
                    while index < lines.len()
                        && (lines[index].starts_with(' ') || lines[index].trim().is_empty())
                    {
                        block.push(lines[index].trim_start());
                        index += 1;
                    }
                    feed.release_notes = Some(block.join("\n").trim_end().to_owned());
                } else {
                    feed.release_notes = Some(scalar(value));
                }
            }
            "files" => {
                while index < lines.len() && lines[index].starts_with(' ') {
                    let item = lines[index].trim_start();
                    index += 1;
                    let (fresh, item) = match item.strip_prefix("- ") {
                        Some(item) => (true, item),
                        None => (false, item),
                    };
                    if fresh {
                        feed.files.push(FeedFile::default());
                    }
                    let (Some(file), Some((key, value))) =
                        (feed.files.last_mut(), item.split_once(':'))
                    else {
                        continue;
                    };
                    let value = scalar(value.trim());
                    match key.trim() {
                        "url" => file.url = value,
                        "sha512" => file.sha512 = value,
                        "size" => file.size = value.parse().ok(),
                        "isAdminRightsRequired" => file.admin = value == "true",
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if feed.files.is_empty()
        && let (Some(url), Some(sha512)) = (path, sha512)
    {
        feed.files.push(FeedFile {
            url,
            sha512,
            size: None,
            admin: false,
        });
    }
    if feed.version.is_empty() {
        return Err("The update feed names no version.".to_owned());
    }
    Ok(feed)
}

fn scalar(value: &str) -> String {
    let value = value.trim();
    for quote in ['\'', '"'] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner.to_owned();
        }
    }
    value.to_owned()
}

/// The installer electron-updater takes on each system: the `.exe` on
/// Windows (`NsisUpdater`), the `.AppImage` on Linux (`AppImageUpdater`).
pub fn installer<'a>(feed: &'a Feed, platform: &str) -> Option<&'a FeedFile> {
    let extension = match platform {
        "win32" => ".exe",
        "linux" => ".AppImage",
        _ => return None,
    };
    feed.files.iter().find(|file| {
        let path = file.url.split(['?', '#']).next().unwrap_or_default();
        path.to_ascii_lowercase()
            .ends_with(&extension.to_ascii_lowercase())
    })
}

/// Whether the downloaded bytes are the file the feed describes: its
/// SHA-512 (base64, as electron-builder writes it) and, where the feed
/// gives it, its size.
pub fn verify(bytes_sha512: &[u8], length: u64, file: &FeedFile) -> Result<(), String> {
    if let Some(size) = file.size
        && size != length
    {
        return Err(format!(
            "The downloaded update is {length} bytes; the feed says {size}."
        ));
    }
    let expected = base64::engine::general_purpose::STANDARD
        .decode(file.sha512.trim())
        .map_err(|_| "The update feed's checksum cannot be read.".to_owned())?;
    if expected != bytes_sha512 {
        return Err(
            "The downloaded update does not match the feed's checksum (sha512).".to_owned(),
        );
    }
    Ok(())
}

/// A running SHA-512, fed as the download arrives.
pub fn hasher() -> Sha512 {
    Sha512::new()
}
