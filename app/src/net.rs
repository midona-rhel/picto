//! Makes the requests that do not go to a library: the update feed and its
//! downloads (`updates`), the pages of the search engines (`search_by_image`)
//! and the icons of the services (`site_icons`).
//!
//! A normal run sends each request to its real address. A run that a check
//! drives never does. When `PICTO_GPUI_NET_ORIGIN` is set, every address is
//! changed to one on that loopback origin: the host of the address becomes
//! the first part of the path, and a redirect that leaves the origin is not
//! followed. When any other variable of `seams.rs` is set, or in a test
//! build, no request is sent at all.
use std::future::Future;

/// Says where the requests of this run are sent.
#[derive(Clone, Debug, PartialEq)]
pub enum Reach {
    /// Each request is sent to its real address.
    Real,
    /// Every request is sent to this loopback origin, where a check serves
    /// the answers.
    Local(String),
    /// No request is sent. This is the case in a test build and in a run that
    /// a check drives without `PICTO_GPUI_NET_ORIGIN`.
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

/// Returns the address to request for `url` in this run, or `None` when this
/// run sends no requests.
pub fn address(url: &str) -> Option<String> {
    match reach() {
        Reach::Real => Some(url.to_owned()),
        Reach::Local(origin) => Some(relocate(url, &origin)),
        Reach::Nowhere => None,
    }
}

/// Returns `url` moved onto `origin`: `https://host/path?query` becomes
/// `<origin>/host/path?query`. An address that is already on the origin, or
/// that cannot be parsed, is returned unchanged.
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

/// Builds the HTTP client for this run. It follows up to ten redirects and
/// fails on the eleventh. When a check serves the answers from a loopback
/// origin, it does not follow a redirect that leaves that origin.
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

/// Runs `work` until it finishes, on the calling thread. The calling thread
/// waits for the whole time, so it must be a background thread.
pub fn block_on<F: Future>(work: F) -> Result<F::Output, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    Ok(runtime.block_on(work))
}

/// Opens `url` in the user's browser, for example a page of search results or
/// a release page. When a check serves the answers from a loopback origin,
/// the page is not opened: its address is printed on standard error and, in a
/// debug build, kept for [`opened`].
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

/// Returns the addresses that [`open_in_browser`] kept instead of opening
/// them in the browser, oldest first.
#[cfg(debug_assertions)]
pub fn opened() -> Vec<String> {
    OPENED
        .lock()
        .map(|opened| opened.clone())
        .unwrap_or_default()
}
