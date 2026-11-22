//! Reads what GitHub and a release's feed say about Picto's releases. The
//! update check reads GitHub's list of releases and takes the newest release
//! of the app that is newer than this version. On Windows and Linux it then
//! reads that release's feed file, `latest.yml` or `latest-linux.yml`, which
//! names the installer, its SHA-512 and its size.
//!
//! The feed has the format that electron-builder writes and that
//! electron-updater reads. `package.sh` writes it in that format, because an
//! Electron-based Picto (version 0.6.19 or earlier) reads the same file with
//! electron-updater when it updates itself to this app.
use base64::Engine as _;
use serde_json::Value;
use sha2::{Digest as _, Sha512};

/// The address of GitHub's list of Picto's 20 newest releases, as JSON.
pub const RELEASES_API: &str =
    "https://api.github.com/repos/midona-rhel/picto/releases?per_page=20";
/// The address of the web page that lists Picto's releases.
pub const RELEASES_PAGE: &str = "https://github.com/midona-rhel/picto/releases";
/// The start of the address of every file of a release. The release's tag
/// and the file's name follow it.
pub const RELEASE_DOWNLOADS: &str = "https://github.com/midona-rhel/picto/releases/download";

/// Holds one release from GitHub's list of releases.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Release {
    pub tag: String,
    pub name: Option<String>,
    pub body: String,
    pub published_at: Option<String>,
    pub html_url: Option<String>,
}

/// Returns the first release in `releases` that is a release of the app, is
/// not a draft, and is newer than the version `current`. GitHub lists the
/// newest release first. A release of the app is one whose tag
/// `is_app_tag` accepts.
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

/// Returns whether `tag` is the tag of a release of the app: three numbers
/// separated by dots, with or without a `v` before them, and optionally a
/// pre-release part after `-` and a build part after `+`. `v1.2.3` and
/// `v0.6.1-alpha` are such tags. The repository also has releases that are
/// not the app, with tags such as `ai-models-v2`.
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

/// Splits a version at each `.` and `-`, after removing a `v` at its start.
/// A part made of digits only becomes a number, and any other part becomes
/// a text.
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

/// Returns whether the version `candidate` is newer than `current`. The
/// versions are compared part by part (`parts`). A missing part counts as
/// the number 0. A number is newer than a text, so `0.6.1` is newer than
/// `0.6.1-alpha`. Two texts are compared as text.
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

/// Returns the name of the feed file among a release's files for the
/// operating system `platform`: `latest.yml` for Windows and
/// `latest-linux.yml` for Linux. Returns `None` for macOS, where the app
/// reads no feed and opens the release's page.
pub fn feed_file(platform: &str) -> Option<&'static str> {
    match platform {
        "win32" => Some("latest.yml"),
        "linux" => Some("latest-linux.yml"),
        _ => None,
    }
}

/// Returns the address of the file `file` of the release tagged `tag`:
/// `RELEASE_DOWNLOADS`, then the tag, then the file's name.
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

/// Holds what a feed file says about an update.
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
    /// The feed's `isAdminRightsRequired`. When it is true, the Windows
    /// installer is started with administrator rights.
    pub admin: bool,
}

impl FeedFile {
    /// Returns the installer's file name, which is the last part of the path
    /// of `url`. Returns `None` when that part is empty, is `..`, or holds a
    /// backslash.
    pub fn name(&self) -> Option<String> {
        self.url
            .split(['?', '#'])
            .next()
            .and_then(|path| path.rsplit('/').next())
            .map(str::to_owned)
            .filter(|name| !name.is_empty() && !name.contains(['/', '\\']) && name != "..")
    }
}

/// Reads a feed file. It reads only the parts of YAML that electron-builder
/// writes in a feed: `key: value` lines at the top level, the list under
/// `files:`, release notes written as a block (`|` or `>`), and, in a feed
/// without `files`, the top-level `path` and `sha512` that older feeds have.
/// Returns an error when the feed names no version.
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

/// Returns the entry of the feed that is the installer for the operating
/// system `platform`: the first file whose name ends in `.exe` for Windows,
/// or in `.AppImage` for Linux, ignoring case. Returns `None` for macOS.
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

/// Checks that a downloaded file is the one that the feed describes.
/// `bytes_sha512` is the SHA-512 of the file and `length` is its size in
/// bytes. The SHA-512 must be equal to the feed's, which is written in
/// base64. The size must be equal to the feed's when the feed gives one.
/// Returns an error that says what differs.
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

/// Returns a new SHA-512 hasher. The caller gives it the bytes of a download
/// as they arrive.
pub fn hasher() -> Sha512 {
    Sha512::new()
}
