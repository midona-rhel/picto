//! The app's network features checked end to end in the running app, on
//! whatever system it runs on (debug builds only):
//! `PICTO_GPUI_NET_CHECK=<folder>` with
//! `PICTO_GPUI_NET_ORIGIN=http://127.0.0.1:<port>`.
//!
//! The check serves, at that origin, what the features would reach: GitHub's
//! list of releases with one newer application release, its `latest.yml`
//! and `latest-linux.yml` and a 3 MB installer; each search engine's page
//! with its upload control and results page, recording the bytes, name and
//! type of every file uploaded to it; and services' icons (a touch icon, a
//! favicon, a page where an icon should be, nothing). Then it runs:
//!
//! - the services' icons through `site_icons`: two found and kept in the
//!   cache, two not (the globe);
//! - an update check through `updates` as this system runs it: on Windows
//!   and Linux the installer downloaded and verified against the feed, then
//!   a feed whose checksum does not match, which must end in an error with
//!   no installer kept; on macOS the release page offered;
//! - a search on every engine through `search_by_image`, in its hidden web
//!   view: nothing reaches an engine before the search starts, the engine
//!   receives exactly the file's bytes, name and type, and the results
//!   address is production's.
//!
//! It writes `net-check.log`, whose last line is `result: pass` or
//! `result: fail`, to the folder, then the app quits.
use crate::grid::menu::{ENGINES, Engine};
use base64::Engine as _;
use gpui::*;
use sha2::{Digest as _, Sha256, Sha512};
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RELEASE: &str = "0.7.0-alpha";
const ICONS: [(&str, bool); 4] = [
    ("danbooru.donmai.us", true),
    ("e621.net", true),
    ("kemono.cr", false),
    ("absent.example", false),
];

/// A file an engine received.
#[derive(Clone, Debug)]
struct Received {
    engine: String,
    name: String,
    mime: String,
    sha256: String,
}

#[derive(Clone)]
struct Server {
    origin: String,
    installer: Arc<Vec<u8>>,
    bad_checksum: Arc<AtomicBool>,
    received: Arc<Mutex<Vec<Received>>>,
    /// Every request's path, in order.
    requests: Arc<Mutex<Vec<String>>>,
}

struct Finished;
impl Global for Finished {}

/// Starts the check if `PICTO_GPUI_NET_CHECK` names a folder.
pub fn start(cx: &mut App) {
    let Some(out) = crate::seams::net_check() else {
        return;
    };
    let _ = std::fs::create_dir_all(&out);
    let mut log = Vec::new();
    let Some(origin) = crate::seams::net_origin() else {
        log.push("PICTO_GPUI_NET_ORIGIN must name a loopback http origin".to_owned());
        finish(&out, &log, false);
        cx.quit();
        return;
    };
    let server = match serve(&origin) {
        Ok(server) => server,
        Err(error) => {
            log.push(format!(
                "the fixtures could not be served at {origin}: {error}"
            ));
            finish(&out, &log, false);
            cx.quit();
            return;
        }
    };
    // Always ready: WebView2 runs the thread's messages while its web view
    // is being made.
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(3))
                .await;
            if cx.update(|cx| cx.try_global::<Finished>().is_some()) {
                break;
            }
        }
    })
    .detach();
    cx.spawn(async move |cx| {
        let mut report = Report { lines: log };
        report.line(&format!(
            "system {} {}; fixtures at {origin}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        let passed = run(&server, &out, &mut report, cx).await;
        finish(&out, &report.lines, passed);
        cx.update(|cx| {
            cx.set_global(Finished);
            cx.quit();
        });
    })
    .detach();
}

struct Report {
    lines: Vec<String>,
}

impl Report {
    fn line(&mut self, line: &str) {
        eprintln!("picto: net check: {line}");
        self.lines.push(line.to_owned());
    }

    fn expect(&mut self, ok: bool, what: &str) -> bool {
        self.line(&format!("{} {what}", if ok { "ok  " } else { "FAIL" }));
        ok
    }
}

fn finish(out: &std::path::Path, lines: &[String], passed: bool) {
    let mut text = lines.join("\n");
    text.push_str(if passed {
        "\nresult: pass\n"
    } else {
        "\nresult: fail\n"
    });
    let _ = std::fs::write(out.join("net-check.log"), text);
}

async fn pause(cx: &mut AsyncApp, ms: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(ms))
        .await;
}

async fn run(
    server: &Server,
    out: &std::path::Path,
    report: &mut Report,
    cx: &mut AsyncApp,
) -> bool {
    let mut passed = true;

    // ── Icons ──
    for (domain, _) in ICONS {
        cx.update(|cx| crate::site_icons::get(domain, cx));
    }
    let since = Instant::now();
    loop {
        let settled = cx.update(|cx| {
            ICONS
                .iter()
                .all(|(domain, _)| crate::site_icons::settled(domain, cx).is_some())
        });
        if settled || since.elapsed() > Duration::from_secs(30) {
            break;
        }
        pause(cx, 100).await;
    }
    for (domain, expected) in ICONS {
        let (found, cached) = cx.update(|cx| {
            (
                crate::site_icons::settled(domain, cx),
                crate::site_icons::cache_file(domain, cx).is_some_and(|file| file.exists()),
            )
        });
        let decodes = cx.update(|cx| {
            crate::site_icons::get(domain, cx)
                .is_some_and(|icon| icon.to_image_data(cx.svg_renderer()).is_ok())
        });
        passed &= report.expect(
            found == Some(expected) && cached == expected && decodes == expected,
            &format!("icon of {domain}: found {found:?}, kept {cached}, decodes {decodes}"),
        );
    }

    // ── Updates ──
    // A first start of this version shows its bundled notes, which hold
    // checks back until they are read.
    let state = cx.update(crate::updates::state);
    passed &= report.expect(
        state.status == crate::updates::Status::Installed
            && state.version.as_deref() == Some(crate::updates::VERSION)
            && !state.release_notes.trim().is_empty(),
        &format!(
            "this version's notes shown at start: {:?} {:?}",
            state.status, state.version
        ),
    );
    cx.update(crate::updates::acknowledge_installed);
    let acknowledged = crate::state::location()
        .and_then(|folder| {
            std::fs::read_to_string(folder.join("acknowledged-update-release.json")).ok()
        })
        .is_some_and(|text| text.contains(crate::updates::VERSION));
    passed &= report.expect(
        cx.update(crate::updates::state).status == crate::updates::Status::Current && acknowledged,
        "notes marked as read",
    );
    let state = check_update(cx).await;
    let platform = state.platform;
    report.line(&format!("update flow of {platform}"));
    if platform == "darwin" {
        passed &= report.expect(
            state.status == crate::updates::Status::Available
                && state.version.as_deref() == Some(RELEASE)
                && state.release_url.ends_with(&format!("/tag/v{RELEASE}")),
            &format!("update offered as its release page: {state:?}"),
        );
    } else {
        let kept = state
            .downloaded
            .as_ref()
            .and_then(|path| std::fs::read(path).ok());
        passed &= report.expect(
            state.status == crate::updates::Status::Downloaded
                && kept.as_deref() == Some(server.installer.as_slice()),
            &format!(
                "installer downloaded and verified: {:?} {:?} {:?}",
                state.status, state.downloaded, state.error
            ),
        );
        // The same release whose feed no longer matches its installer.
        server.bad_checksum.store(true, Ordering::SeqCst);
        let folder = state
            .downloaded
            .as_ref()
            .and_then(|path| path.parent().map(PathBuf::from));
        let state = check_update(cx).await;
        let left = folder
            .and_then(|folder| std::fs::read_dir(folder).ok())
            .map(|entries| entries.count())
            .unwrap_or(0);
        passed &= report.expect(
            state.status == crate::updates::Status::Error
                && state.downloaded.is_none()
                && left == 0,
            &format!(
                "mismatching installer refused and not kept: {:?} {:?}, {left} files left",
                state.status, state.error
            ),
        );
    }

    // ── Search by Image ──
    let engine_requests = |server: &Server| {
        server
            .requests
            .lock()
            .map(|requests| {
                requests
                    .iter()
                    .filter(|path| {
                        ENGINE_PAGES
                            .iter()
                            .any(|(_, page, _)| path.starts_with(page))
                    })
                    .count()
            })
            .unwrap_or(0)
    };
    passed &= report.expect(
        engine_requests(server) == 0,
        "no engine reached before a search starts",
    );
    let sample = out.join("sample.png");
    let bytes = png([200, 120, 40]);
    let _ = std::fs::write(&sample, &bytes);
    let digest = hex(&Sha256::digest(&bytes));
    for (engine, _) in ENGINES {
        let before = server
            .received
            .lock()
            .map(|received| received.len())
            .unwrap_or(0);
        let task = cx.update(|cx| {
            crate::search_by_image::results_address(
                engine,
                crate::search_by_image::Upload {
                    path: sample.clone(),
                    mime: "image/png".into(),
                },
                cx,
            )
        });
        let result = task.await;
        let received: Vec<Received> = server
            .received
            .lock()
            .map(|received| received[before..].to_vec())
            .unwrap_or_default();
        let key = engine_key(engine);
        let expected = expected_result(engine, &server.origin);
        let exact = received.len() == 1
            && received[0].engine == key
            && received[0].name == "sample.png"
            && received[0].mime == "image/png"
            && received[0].sha256 == digest;
        let address_ok = match (&result, &expected) {
            (Ok(url), Expected::Exact(expected)) => url == expected,
            (Ok(url), Expected::Prefix(prefix)) => url.starts_with(prefix),
            _ => false,
        };
        passed &= report.expect(
            exact && address_ok,
            &format!("{key}: {result:?}; received {received:?}"),
        );
    }
    passed
}

/// A check from Check Now, followed to its end.
async fn check_update(cx: &mut AsyncApp) -> crate::updates::State {
    use crate::updates::Status;
    cx.update(crate::updates::check);
    let since = Instant::now();
    loop {
        pause(cx, 100).await;
        let state = cx.update(crate::updates::state);
        let settled = !matches!(state.status, Status::Checking | Status::Downloading)
            && !(state.status == Status::Available && !state.mac());
        if settled || since.elapsed() > Duration::from_secs(120) {
            return state;
        }
    }
}

enum Expected {
    Exact(String),
    Prefix(String),
}

fn engine_key(engine: Engine) -> &'static str {
    match engine {
        Engine::TinEye => "tineye",
        Engine::SauceNao => "saucenao",
        Engine::Yandex => "yandex",
        Engine::Sogou => "sogou",
        Engine::Bing => "bing",
    }
}

fn expected_result(engine: Engine, origin: &str) -> Expected {
    match engine {
        Engine::SauceNao => Expected::Exact(format!(
            "https://saucenao.com/search.php?db=999&url={}",
            url::form_urlencoded::byte_serialize(
                format!("{origin}/saucenao.com/userdata/abc.png").as_bytes()
            )
            .collect::<String>()
        )),
        other => Expected::Prefix(format!(
            "{origin}{}",
            ENGINE_PAGES
                .iter()
                .find(|(key, _, _)| *key == engine_key(other))
                .map(|(_, _, result)| *result)
                .unwrap_or_default()
        )),
    }
}

/// Each engine's page and the results address its upload leads to.
const ENGINE_PAGES: [(&str, &str, &str); 5] = [
    ("tineye", "/tineye.com/", "/tineye.com/search/abc123"),
    ("saucenao", "/saucenao.com/", "/saucenao.com/search.php"),
    (
        "yandex",
        "/yandex.com/images/",
        "/yandex.com/images/search?rpt=imageview&cbir_id=42",
    ),
    (
        "sogou",
        "/pic.sogou.com/",
        "/pic.sogou.com/ris?query=x&flag=1",
    ),
    (
        "bing",
        "/www.bing.com/images",
        "/www.bing.com/search?view=detailv2&iss=sbiupload&bcid=XYZ",
    ),
];

/// The engine's page: its upload control where production's script looks
/// for it, behind a button where the engine hides it, and an upload that
/// leads to the results address.
fn engine_page(key: &str, result: &str) -> String {
    let control = match key {
        "tineye" => r#"<div id="upload-box"><input type="file" id="f"></div>"#.to_owned(),
        "saucenao" => r#"<form id="searchForm" method="post" enctype="multipart/form-data" action="/saucenao.com/search.php"><input type="file" id="fileInput" name="file"></form>"#.to_owned(),
        "yandex" => r#"<button class="input__cbir-button" id="b">image</button>"#.to_owned(),
        "sogou" => r#"<span id="cameraIco">camera</span>"#.to_owned(),
        _ => r#"<div id="sb_sbi">camera</div>"#.to_owned(),
    };
    let script = match key {
        "tineye" => format!(
            "document.getElementById('f').addEventListener('change', e => send(e.target, '{result}'));"
        ),
        "saucenao" => String::new(),
        "yandex" | "sogou" | "bing" => {
            let (button, id) = match key {
                "yandex" => ("b", ""),
                "sogou" => ("cameraIco", ""),
                _ => ("sb_sbi", "i.id = 'sb_fileinput';"),
            };
            format!(
                "document.getElementById('{button}').addEventListener('click', () => {{ const i = document.createElement('input'); i.type = 'file'; {id} i.addEventListener('change', e => send(e.target, '{result}')); document.body.appendChild(i); }});"
            )
        }
        _ => String::new(),
    };
    format!(
        "<!doctype html><html><head><title>{key}</title></head><body>{control}<script>
async function send(input, next) {{
  const file = input.files[0];
  const body = await file.arrayBuffer();
  await fetch('/upload/{key}?name=' + encodeURIComponent(file.name) + '&type=' + encodeURIComponent(file.type), {{ method: 'POST', body }});
  location.href = next;
}}
{script}
</script></body></html>"
    )
}

fn serve(origin: &str) -> std::io::Result<Server> {
    let address = origin.trim_start_matches("http://");
    let listener = TcpListener::bind(address)?;
    let server = Server {
        origin: origin.to_owned(),
        installer: Arc::new(
            (0..3 * 1024 * 1024_u32)
                .map(|index| (index * 7 % 251) as u8)
                .collect(),
        ),
        bad_checksum: Arc::new(AtomicBool::new(false)),
        received: Arc::default(),
        requests: Arc::default(),
    };
    let shared = server.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let server = shared.clone();
            std::thread::spawn(move || answer(stream, &server));
        }
    });
    Ok(server)
}

fn answer(mut stream: TcpStream, server: &Server) {
    let Some((method, path, content_type, body)) = read_request(&mut stream) else {
        return;
    };
    if let Ok(mut requests) = server.requests.lock() {
        requests.push(path.clone());
    }
    let (bare, query) = path.split_once('?').unwrap_or((&path, ""));
    let html = |body: &str| {
        (
            "200 OK",
            "text/html; charset=utf-8",
            body.as_bytes().to_vec(),
        )
    };
    let (status, kind, content): (&str, &str, Vec<u8>) = match (method.as_str(), bare) {
        ("POST", upload) if upload.starts_with("/upload/") => {
            let field = |name: &str| {
                url::form_urlencoded::parse(query.as_bytes())
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.into_owned())
                    .unwrap_or_default()
            };
            record(
                server,
                &upload["/upload/".len()..],
                &field("name"),
                &field("type"),
                &body,
            );
            ("200 OK", "text/plain", b"ok".to_vec())
        }
        ("POST", "/saucenao.com/search.php") => {
            if let Some((name, mime, data)) = multipart_file(&content_type, &body) {
                record(server, "saucenao", &name, &mime, &data);
            }
            html(
                r##"<html><body><div id="yourimage"><a href="#"><img src="/saucenao.com/userdata/abc.png"></a></div></body></html>"##,
            )
        }
        ("GET", "/saucenao.com/userdata/abc.png") => ("200 OK", "image/png", png([1, 2, 3])),
        ("GET", "/api.github.com/repos/midona-rhel/picto/releases") => {
            let releases = serde_json::json!([
                { "draft": false, "tag_name": "ai-models-v3", "name": "Picto AI Models v3" },
                { "draft": true, "tag_name": "v9.0.0", "name": "Draft" },
                {
                    "draft": false,
                    "tag_name": format!("v{RELEASE}"),
                    "name": format!("Picto {RELEASE}"),
                    "body": "## Highlights\n\n- Faster **imports**\n- Smoother grid scrolling",
                    "published_at": "2026-11-01T10:00:00Z",
                    "html_url": format!("{}/github.com/midona-rhel/picto/releases/tag/v{RELEASE}", server.origin),
                },
                { "draft": false, "tag_name": "v0.6.0-alpha", "name": "Old" },
            ]);
            (
                "200 OK",
                "application/json",
                releases.to_string().into_bytes(),
            )
        }
        ("GET", asset)
            if asset.starts_with(&format!(
                "/github.com/midona-rhel/picto/releases/download/v{RELEASE}/"
            )) =>
        {
            let name = asset.rsplit('/').next().unwrap_or_default();
            let installer = match name {
                "latest.yml" => Some(format!("Picto-Setup-{RELEASE}.exe")),
                "latest-linux.yml" => Some(format!("Picto-{RELEASE}.AppImage")),
                _ => None,
            };
            if let Some(installer) = installer {
                let digest = if server.bad_checksum.load(Ordering::SeqCst) {
                    Sha512::digest(b"another installer")
                } else {
                    Sha512::digest(server.installer.as_slice())
                };
                let sha = base64::engine::general_purpose::STANDARD.encode(digest);
                let size = server.installer.len();
                let feed = format!(
                    "version: {RELEASE}\nfiles:\n  - url: {installer}\n    sha512: {sha}\n    size: {size}\n  - url: {installer}.blockmap\n    sha512: AAAA\n    size: 10\npath: {installer}\nsha512: {sha}\nreleaseDate: '2026-11-01T10:00:00.000Z'\n"
                );
                ("200 OK", "text/yaml", feed.into_bytes())
            } else if name == format!("Picto-Setup-{RELEASE}.exe")
                || name == format!("Picto-{RELEASE}.AppImage")
            {
                (
                    "200 OK",
                    "application/octet-stream",
                    server.installer.to_vec(),
                )
            } else {
                ("404 Not Found", "text/plain", Vec::new())
            }
        }
        ("GET", "/danbooru.donmai.us/apple-touch-icon.png") => {
            ("200 OK", "image/png", png([200, 40, 40]))
        }
        ("GET", "/e621.net/favicon.ico") => ("200 OK", "image/x-icon", ico([30, 160, 60])),
        ("GET", "/kemono.cr/apple-touch-icon.png") => html("<html>not an icon</html>"),
        ("GET", page) => match ENGINE_PAGES.iter().find(|(_, start, _)| *start == page) {
            Some((key, _, result)) => html(&engine_page(key, result)),
            None if ENGINE_PAGES
                .iter()
                .any(|(_, _, result)| result.split('?').next() == Some(page)) =>
            {
                html("<html><body>results</body></html>")
            }
            None => ("404 Not Found", "text/plain", Vec::new()),
        },
        _ => ("404 Not Found", "text/plain", Vec::new()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        content.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&content);
    let _ = stream.flush();
}

fn record(server: &Server, engine: &str, name: &str, mime: &str, body: &[u8]) {
    if let Ok(mut received) = server.received.lock() {
        received.push(Received {
            engine: engine.to_owned(),
            name: name.to_owned(),
            mime: mime.to_owned(),
            sha256: hex(&Sha256::digest(body)),
        });
    }
}

/// The file part of a `multipart/form-data` body: its name, type and bytes.
fn multipart_file(content_type: &str, body: &[u8]) -> Option<(String, String, Vec<u8>)> {
    let boundary = format!("--{}", content_type.split("boundary=").nth(1)?.trim());
    let parts = split(body, boundary.as_bytes());
    parts.into_iter().find_map(|part| {
        let end = part.windows(4).position(|window| window == b"\r\n\r\n")?;
        let head = String::from_utf8_lossy(&part[..end]);
        let name = head
            .split("filename=\"")
            .nth(1)?
            .split('"')
            .next()?
            .to_owned();
        let mime = head
            .split("\r\n")
            .find_map(|line| line.strip_prefix("Content-Type: "))
            .unwrap_or_default()
            .to_owned();
        let data = &part[end + 4..];
        Some((
            name,
            mime,
            data.strip_suffix(b"\r\n").unwrap_or(data).to_vec(),
        ))
    })
}

fn split<'a>(body: &'a [u8], boundary: &[u8]) -> Vec<&'a [u8]> {
    let mut parts = Vec::new();
    let mut rest = body;
    while let Some(at) = rest
        .windows(boundary.len())
        .position(|window| window == boundary)
    {
        parts.push(&rest[..at]);
        rest = &rest[at + boundary.len()..];
    }
    parts.push(rest);
    parts
}

/// The request's method, path, `Content-Type` and body.
fn read_request(stream: &mut TcpStream) -> Option<(String, String, String, Vec<u8>)> {
    let mut data = Vec::new();
    let mut buffer = [0u8; 16384];
    let end = loop {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        data.extend_from_slice(&buffer[..read]);
        if let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
        if data.len() > 64 * 1024 {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&data[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let mut content_type = String::new();
    let mut length = 0usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "content-type" => content_type = value.trim().to_owned(),
            "content-length" => length = value.trim().parse().unwrap_or(0),
            _ => {}
        }
    }
    let mut body = data[end..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Some((method, path, content_type, body))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A 32×32 picture of one colour.
fn png(color: [u8; 3]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let image = image::RgbImage::from_pixel(32, 32, image::Rgb(color));
    let _ = image::DynamicImage::ImageRgb8(image).write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    );
    bytes
}

/// An icon file holding one 32×32 PNG with transparency, as favicons do.
fn ico(color: [u8; 3]) -> Vec<u8> {
    let mut png = Vec::new();
    let image =
        image::RgbaImage::from_pixel(32, 32, image::Rgba([color[0], color[1], color[2], 255]));
    let _ = image::DynamicImage::ImageRgba8(image)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    let mut bytes = vec![0, 0, 1, 0, 1, 0, 32, 32, 0, 0, 1, 0, 32, 0];
    bytes.extend_from_slice(&(png.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&22_u32.to_le_bytes());
    bytes.extend(png);
    bytes
}
