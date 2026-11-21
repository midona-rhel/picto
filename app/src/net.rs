//! The requests the app makes outside a library: the update feed and its
//! downloads (`updates`), the search engines' pages (`search_by_image`) and
//! the services' icons (`site_icons`).
//!
//! A real run reaches the real addresses. A run under a check never does:
//! with `PICTO_GPUI_NET_ORIGIN` every address moves onto that loopback
//! origin (its host becomes the first part of the path, and a redirect
//! leaving the origin is not followed); with any other seam, or in a test
//! build, nothing is fetched at all.
use std::future::Future;

/// Where this run's requests go.
#[derive(Clone, Debug, PartialEq)]
pub enum Reach {
    /// The real addresses.
    Real,
    /// That loopback origin, for the checks.
    Local(String),
    /// Nowhere: a check without local fixtures.
    Nowhere,
}

pub fn reach() -> Reach {
    if let Some(origin) = crate::seams::net_origin() {
        return Reach::Local(origin);
    }
    if crate::seams::automation().is_some() {
        return Reach::Nowhere;
    }
    Reach::Real
}

/// The address to fetch for `url` in this run, if any.
pub fn address(url: &str) -> Option<String> {
    match reach() {
        Reach::Real => Some(url.to_owned()),
        Reach::Local(origin) => Some(relocate(url, &origin)),
        Reach::Nowhere => None,
    }
}

/// `https://host/path?query` as `<origin>/host/path?query`. An address
/// already on the origin is kept.
pub fn relocate(url: &str, origin: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return url.to_owned();
    };
    if parsed.origin().ascii_serialization() == origin {
        return url.to_owned();
    }
    let host = parsed.host_str().unwrap_or_default();
    let mut moved = format!("{origin}/{host}{}", parsed.path());
    if let Some(query) = parsed.query() {
        moved.push('?');
        moved.push_str(query);
    }
    moved
}

/// A client for this run: redirects are followed (up to ten, as `fetch`
/// does), but never off the local origin of a check.
pub fn client() -> Result<reqwest::Client, String> {
    let local = match reach() {
        Reach::Local(origin) => Some(origin),
        _ => None,
    };
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error("too many redirects");
            }
            match &local {
                Some(origin) if attempt.url().origin().ascii_serialization() != *origin => {
                    attempt.stop()
                }
                _ => attempt.follow(),
            }
        }))
        .build()
        .map_err(|error| error.to_string())
}

/// Runs `work` to the end on the calling thread, which must be a
/// background one.
pub fn block_on<F: Future>(work: F) -> Result<F::Output, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    Ok(runtime.block_on(work))
}

/// A page for the user in the browser (a results page, a release page);
/// under a check, written on standard error (and kept for the check,
/// [`opened`]) instead of opened.
pub fn open_in_browser(url: &str, cx: &mut gpui::App) {
    if matches!(reach(), Reach::Local(_)) {
        eprintln!("picto: would open in the browser: {url}");
        #[cfg(debug_assertions)]
        if let Ok(mut opened) = OPENED.lock() {
            opened.push(url.to_owned());
        }
    } else {
        crate::desktop::open_link(url, cx);
    }
}

#[cfg(debug_assertions)]
static OPENED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The pages a check would have opened in the browser, oldest first.
#[cfg(debug_assertions)]
pub fn opened() -> Vec<String> {
    OPENED
        .lock()
        .map(|opened| opened.clone())
        .unwrap_or_default()
}
