//! Gets the icon of each service. An icon is requested from the site of the
//! service: first its touch icon (`/apple-touch-icon.png`), then its favicon
//! (`/favicon.ico`). The icon is kept in `site-icons/` in this app's folder,
//! so that it is requested only once. A service without an icon is drawn with
//! the globe icon.
//!
//! The requests are made off the main thread, and only one request per
//! service runs at a time. A service for which no icon was found is asked
//! again after 30 seconds, as long as a view still shows it.
use base64::Engine as _;
use gpui::{App, AppContext as _, Context, Entity, Global, Image, ImageFormat};
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The largest icon that is accepted, in bytes (512 KB).
const MAX_ICON_BYTES: usize = 512 * 1024;
/// The paths that are requested on the site of a service, in this order.
const ICON_PATHS: [&str; 2] = ["/apple-touch-icon.png", "/favicon.ico"];
/// The time after which one request is given up.
const TIMEOUT: Duration = Duration::from_secs(7);
/// The time after which a service without an icon is asked again.
const RETRY: Duration = Duration::from_secs(30);
const ACCEPT: &str =
    "image/png,image/vnd.microsoft.icon,image/x-icon,image/webp,image/jpeg,image/gif";

enum Slot {
    Fetching,
    Found(Arc<Image>),
    Missing(Instant),
}

/// Holds the icons known in this run, by domain. A view that shows icons
/// observes this entity and is drawn again when an icon arrives.
pub struct SiteIcons {
    slots: HashMap<String, Slot>,
    cache: Option<PathBuf>,
}

struct Shared(Entity<SiteIcons>);

impl Global for Shared {}

/// Returns the entity that holds the app's icons. It is created on first use.
pub fn entity(cx: &mut App) -> Entity<SiteIcons> {
    if let Some(shared) = cx.try_global::<Shared>() {
        return shared.0.clone();
    }
    let cache = crate::state::location().map(|folder| folder.join("site-icons"));
    let icons = cx.new(|_| SiteIcons {
        slots: HashMap::new(),
        cache,
    });
    cx.set_global(Shared(icons.clone()));
    icons
}

/// Returns the icon of the service when it is known. Otherwise it returns
/// `None`, for which the caller draws the globe icon, and it starts
/// requesting the icon, unless a request is under way or one failed less than
/// 30 seconds ago.
pub fn get(domain: &str, cx: &mut App) -> Option<Arc<Image>> {
    let domain = normalize(domain)?;
    let icons = entity(cx);
    icons.update(cx, |icons, cx| {
        match icons.slots.get(&domain) {
            Some(Slot::Found(image)) => return Some(image.clone()),
            Some(Slot::Fetching) => return None,
            Some(Slot::Missing(since)) if since.elapsed() < RETRY => return None,
            _ => {}
        }
        icons.slots.insert(domain.clone(), Slot::Fetching);
        icons.fetch(domain, cx);
        None
    })
}

/// Returns `Some(true)` when the icon of the service was found, `Some(false)`
/// when it was not found, and `None` while it is still being requested. The
/// network check uses this.
#[cfg(debug_assertions)]
pub fn settled(domain: &str, cx: &mut App) -> Option<bool> {
    let domain = normalize(domain)?;
    match entity(cx).read(cx).slots.get(&domain)? {
        Slot::Found(_) => Some(true),
        Slot::Missing(_) => Some(false),
        Slot::Fetching => None,
    }
}

/// Returns the file in which the icon of `domain` is kept. The network check
/// uses this.
#[cfg(debug_assertions)]
pub fn cache_file(domain: &str, cx: &mut App) -> Option<PathBuf> {
    let folder = entity(cx).read(cx).cache.clone()?;
    Some(folder.join(format!("{}.json", cache_name(&normalize(domain)?))))
}

impl SiteIcons {
    fn fetch(&mut self, domain: String, cx: &mut Context<Self>) {
        let cache = self.cache.clone();
        let work = cx.background_spawn({
            let domain = domain.clone();
            async move { cached_or_fetched(&domain, cache.as_deref()) }
        });
        cx.spawn(async move |this, cx| {
            let found = work.await;
            let missing = found.is_none();
            let _ = this.update(cx, |icons, cx| {
                let slot = match found {
                    Some((format, bytes)) => {
                        Slot::Found(Arc::new(Image::from_bytes(format, bytes)))
                    }
                    None => Slot::Missing(Instant::now()),
                };
                icons.slots.insert(domain, slot);
                cx.notify();
            });
            if missing {
                // A view that still shows the globe icon is drawn again after
                // the delay, and then asks for the icon again.
                cx.background_executor().timer(RETRY).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
    }
}

/// Returns `value` trimmed and in lower case when it is a host name and
/// nothing else, and `None` otherwise.
fn normalize(value: &str) -> Option<String> {
    let domain = value.trim().to_ascii_lowercase();
    let valid = !domain.is_empty()
        && domain.len() <= 253
        && domain.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
        })
        && !domain.contains("..");
    valid.then_some(domain)
}

/// Returns the name of the file that keeps the icon of `domain`: the SHA-256
/// of the domain, in hexadecimal.
fn cache_name(domain: &str) -> String {
    Sha256::digest(domain.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Returns the icon of `domain`: the kept one if there is one, and otherwise
/// the first of the two on the site that is a picture of at most 512 KB. An
/// icon taken from the site is kept for next time.
fn cached_or_fetched(domain: &str, cache: Option<&Path>) -> Option<(ImageFormat, Vec<u8>)> {
    if let Some(found) = cache.and_then(|cache| read_cached(cache, domain)) {
        return Some(found);
    }
    let client = crate::net::client().ok()?;
    for path in ICON_PATHS {
        let Some(url) = crate::net::address(&format!("https://{domain}{path}")) else {
            // A run that sends no requests (`net::reach`) has no icon to
            // show.
            return None;
        };
        let fetched = crate::net::block_on(async {
            let response = client
                .get(&url)
                .header(reqwest::header::ACCEPT, ACCEPT)
                .timeout(TIMEOUT)
                .send()
                .await
                .ok()?;
            if !response.status().is_success() {
                return None;
            }
            if response
                .content_length()
                .is_some_and(|length| length as usize > MAX_ICON_BYTES)
            {
                return None;
            }
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            let bytes = response.bytes().await.ok()?;
            Some((bytes.to_vec(), content_type))
        });
        let Ok(Some((bytes, content_type))) = fetched else {
            continue;
        };
        if bytes.is_empty() || bytes.len() > MAX_ICON_BYTES {
            continue;
        }
        let Some(mime) = detect_mime(&bytes, content_type.as_deref()) else {
            continue;
        };
        if let Some(cache) = cache {
            write_cached(cache, domain, mime, &bytes);
        }
        return Some((format(mime), bytes));
    }
    None
}

fn read_cached(cache: &Path, domain: &str) -> Option<(ImageFormat, Vec<u8>)> {
    let text = std::fs::read_to_string(cache.join(format!("{}.json", cache_name(domain)))).ok()?;
    let record: serde_json::Value = serde_json::from_str(&text).ok()?;
    let mime = record.get("mime")?.as_str()?;
    let data = record.get("data")?.as_str()?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    let mime = MIMES.into_iter().find(|known| *known == mime)?;
    Some((format(mime), bytes))
}

/// Writes the icon as a JSON record with the fields `mime` and `data`, where
/// `data` holds the bytes in Base64. The record is written through a
/// temporary file, so that a reader never sees half of it
/// (`state::write_file`).
fn write_cached(cache: &Path, domain: &str, mime: &str, bytes: &[u8]) {
    let record = serde_json::json!({
        "mime": mime,
        "data": base64::engine::general_purpose::STANDARD.encode(bytes),
    })
    .to_string();
    let name = format!("{}.json", cache_name(domain));
    if let Err(error) = crate::state::write_file(cache, &name, record.as_bytes()) {
        eprintln!("picto: the icon of {domain} could not be kept: {error}");
    }
}

const MIMES: [&str; 5] = [
    "image/png",
    "image/x-icon",
    "image/jpeg",
    "image/gif",
    "image/webp",
];

/// Returns the type of the picture, found from its first bytes, or
/// `image/x-icon` when the declared content type says that it is an icon.
/// Anything else, such as a page, is not an icon and gives `None`.
fn detect_mime(bytes: &[u8], content_type: Option<&str>) -> Option<&'static str> {
    if bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0, 0, 1, 0]) {
        return Some("image/x-icon");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    let declared = content_type?.split(';').next()?.trim().to_ascii_lowercase();
    matches!(
        declared.as_str(),
        "image/vnd.microsoft.icon" | "image/x-icon"
    )
    .then_some("image/x-icon")
}

fn format(mime: &str) -> ImageFormat {
    match mime {
        "image/x-icon" => ImageFormat::Ico,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::Webp,
        _ => ImageFormat::Png,
    }
}
