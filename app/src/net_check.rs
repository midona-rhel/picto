//! A check of the app's network features, run from start to finish in the
//! running app on whatever system it runs on. It exists in debug builds only
//! and is started with `PICTO_GPUI_NET_CHECK=<folder>` together with
//! `PICTO_GPUI_NET_ORIGIN=http://127.0.0.1:<port>`.
//!
//! The check runs a small server at that origin. The server answers with
//! what the features would otherwise fetch from the internet:
//!
//! - GitHub's list of releases, with one release newer than the app, its
//!   `latest.yml` and `latest-linux.yml`, and an installer. The installer is
//!   a harmless stand-in that the check builds itself: on Windows a small
//!   program compiled with the system's C# compiler, elsewhere a shell
//!   script. It only writes what it was started with (its path, its
//!   arguments, the AppImage variables) to `installer-ran.txt` in the folder.
//! - Each search engine's page with its upload control and its results page.
//!   The server records the bytes, name and type of every file uploaded.
//! - Icons for services: one touch icon, one favicon, one page where an
//!   icon should be, and one address that answers with nothing.
//!
//! Then the check runs these steps:
//!
//! 1. The services' icons, through `site_icons`. Two are found and kept in
//!    the cache. Two are not found, and the globe icon is used for them.
//! 2. An update check, through `updates`, in the way this system does it.
//!    On Windows and Linux:
//!    - The installer is downloaded and verified against the feed.
//!    - A feed whose checksum does not match the installer must end in an
//!      error, with no installer kept, and Restart and Install must then
//!      start nothing.
//!    - The good installer is downloaded again and changed on disk after it
//!      was verified. Restart and Install must refuse it, start nothing, and
//!      leave the app running with the error shown.
//!    - The installer is downloaded once more.
//!
//!    On macOS the update is offered as a link to the release page.
//! 3. A search on every engine, through `search_by_image`, in its hidden web
//!    view. Nothing may reach an engine before the search starts. The engine
//!    must receive exactly the file's bytes, name and type. The results
//!    address must be the one the Electron app opens.
//! 4. The install, which is the last step. Restart and Install starts the
//!    stand-in installer the way the Electron app starts its installer, and
//!    quits the app. When the folder holds a file named `install-on-quit`,
//!    the app instead quits with the update waiting, which installs it
//!    silently; the icons and the searches are skipped in that case. On
//!    macOS, Open Download Page writes the address of the release page to
//!    the log. When the check follows another system's steps
//!    (`PICTO_GPUI_UPDATE_PLATFORM`), the installer is verified but not
//!    started.
//!
//! The check writes `net-check.log` to the folder. The log's last line is
//! `result: pass` or `result: fail`. Then the app quits. Whether the
//! stand-in installer ran, and with what, is for the caller to read in
//! `installer-ran.txt` after the app has ended (the workflow's Network
//! features step does this).
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

/// One file that an engine's page received.
#[derive(Clone, Debug)]
struct Received {
    engine: String,
    name: String,
    mime: String,
    sha256: String,
}

/// Where the stand-in installer writes what it was started with.
const MARKER: &str = "installer-ran.txt";

#[derive(Clone)]
struct Server {
    origin: String,
    installer: Arc<Vec<u8>>,
    bad_checksum: Arc<AtomicBool>,
    received: Arc<Mutex<Vec<Received>>>,
    /// The path of every request, in the order the requests arrived.
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
    let installer = match stand_in(&out) {
        Ok(installer) => installer,
        Err(error) => {
            log.push(format!("the stand-in installer could not be made: {error}"));
            finish(&out, &log, false);
            cx.quit();
            return;
        }
    };
    let server = match serve(&origin, installer) {
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
    // This must be ready at all times, because WebView2 runs the thread's
    // messages while its web view is being made.
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
        let on_quit = out.join("install-on-quit").exists();
        let mut passed = run(&server, &out, on_quit, &mut report, cx).await;
        passed &= install(&out, on_quit, passed, &mut report, cx).await;
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
    on_quit: bool,
    report: &mut Report,
    cx: &mut AsyncApp,
) -> bool {
    let mut passed = true;
    if !on_quit {
        passed &= icons(report, cx).await;
    }
    passed &= updates(server, out, report, cx).await;
    if !on_quit {
        passed &= searches(server, out, report, cx).await;
    }
    passed
}

async fn icons(report: &mut Report, cx: &mut AsyncApp) -> bool {
    let mut passed = true;
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

    passed
}

async fn updates(
    server: &Server,
    out: &std::path::Path,
    report: &mut Report,
    cx: &mut AsyncApp,
) -> bool {
    let mut passed = true;
    // On the first start of a version the app shows that version's release
    // notes, and it does not check for updates until the notes have been
    // read.
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
        // The same release, with a feed whose checksum no longer matches the
        // installer.
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
        // There is no installer, so Restart and Install must start nothing.
        cx.update(crate::updates::install);
        pause(cx, 1500).await;
        let state = cx.update(crate::updates::state);
        passed &= report.expect(
            state.status == crate::updates::Status::Error && !out.join(MARKER).exists(),
            "Restart and Install after the refused download started nothing",
        );

        // The good installer is downloaded again and then changed on disk
        // before it is started.
        server.bad_checksum.store(false, Ordering::SeqCst);
        let state = check_update(cx).await;
        let downloaded = state.downloaded.clone();
        passed &= report.expect(
            state.status == crate::updates::Status::Downloaded && downloaded.is_some(),
            &format!(
                "installer downloaded again: {:?} {:?}",
                state.status, state.error
            ),
        );
        if let Some(path) = &downloaded
            && let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(path)
        {
            let _ = file.write_all(b"changed");
        }
        cx.update(crate::updates::install);
        pause(cx, 1500).await;
        let state = cx.update(crate::updates::state);
        passed &= report.expect(
            state.status == crate::updates::Status::Error
                && state.error.is_some()
                && downloaded.as_ref().is_some_and(|path| !path.exists())
                && !out.join(MARKER).exists(),
            &format!(
                "installer changed after its verification not started, the app still running, the error shown: {:?} {:?}",
                state.status, state.error
            ),
        );
        let state = check_update(cx).await;
        let kept = state
            .downloaded
            .as_ref()
            .and_then(|path| std::fs::read(path).ok());
        passed &= report.expect(
            state.status == crate::updates::Status::Downloaded
                && kept.as_deref() == Some(server.installer.as_slice()),
            &format!(
                "installer downloaded and verified once more: {:?} {:?}",
                state.status, state.error
            ),
        );
    }
    passed
}

async fn searches(
    server: &Server,
    out: &std::path::Path,
    report: &mut Report,
    cx: &mut AsyncApp,
) -> bool {
    let mut passed = true;
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

/// The last step. On Windows and Linux the update is installed in the way
/// that system installs it; the app then quits, which ends the check. On
/// macOS the release page is offered.
async fn install(
    out: &std::path::Path,
    on_quit: bool,
    passed: bool,
    report: &mut Report,
    cx: &mut AsyncApp,
) -> bool {
    use crate::updates::Status;
    let state = cx.update(crate::updates::state);
    if state.mac() {
        cx.update(crate::updates::install);
        let opened = crate::net::opened();
        return report.expect(
            opened
                .last()
                .is_some_and(|url| url.ends_with(&format!("/tag/v{RELEASE}"))),
            &format!("Open Download Page: the release page, {opened:?}"),
        );
    }
    if state.status != Status::Downloaded {
        return report.expect(false, "no update waiting to be installed");
    }
    let system = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        _ => "linux",
    };
    if state.platform != system {
        // The check follows another system's steps: the installer is
        // verified but never started.
        cx.update(crate::updates::install);
        pause(cx, 1500).await;
        let state = cx.update(crate::updates::state);
        return report.expect(
            state.status == Status::Downloaded && !out.join(MARKER).exists(),
            &format!(
                "{} installer verified and not started on {system}",
                state.platform
            ),
        );
    }
    if on_quit {
        report.line("quitting with the update waiting: it installs as the app quits");
        return true;
    }
    // The app quits as soon as the installer has started, so the log is
    // written before that.
    report.line("Restart and Install: the installer starts and the app quits");
    finish(out, &report.lines, passed);
    cx.update(crate::updates::install);
    pause(cx, 15_000).await;
    let state = cx.update(crate::updates::state);
    report.expect(
        false,
        &format!(
            "the app did not quit after Restart and Install: {:?} {:?}",
            state.status, state.error
        ),
    )
}

/// Builds the stand-in installer. Its only effect is to write what it was
/// started with to [`MARKER`] in `out`. On Windows it is a program compiled
/// with the C# compiler of the .NET Framework, which every Windows has.
/// Elsewhere it is a shell script, which stands in for the AppImage and is
/// never run on macOS.
fn stand_in(out: &std::path::Path) -> Result<Vec<u8>, String> {
    let marker = out.join(MARKER);
    let _ = std::fs::remove_file(&marker);
    let marker = marker.to_string_lossy().into_owned();
    if std::env::consts::OS != "windows" {
        if marker.contains('\'') {
            return Err(format!("the folder's path holds a quote: {marker}"));
        }
        return Ok(format!(
            "#!/bin/sh\n{{\n  printf 'path=%s\\n' \"$0\"\n  for argument in \"$@\"; do printf 'arg=%s\\n' \"$argument\"; done\n  printf 'silent=%s\\n' \"$APPIMAGE_SILENT_INSTALL\"\n  printf 'exit-after=%s\\n' \"$APPIMAGE_EXIT_AFTER_INSTALL\"\n}} > '{marker}.part' && mv '{marker}.part' '{marker}'\n"
        )
        .into_bytes());
    }
    let windows = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(PathBuf::from)
        .ok_or("no SystemRoot")?;
    let compiler = ["Framework64", "Framework"]
        .iter()
        .map(|framework| {
            windows
                .join("Microsoft.NET")
                .join(framework)
                .join("v4.0.30319")
                .join("csc.exe")
        })
        .find(|compiler| compiler.exists())
        .ok_or("no C# compiler of the .NET Framework")?;
    let folder = out.join("stand-in");
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let source = folder.join("StandIn.cs");
    let program = folder.join("StandIn.exe");
    let verbatim = marker.replace('"', "\"\"");
    std::fs::write(
        &source,
        format!(
            "using System.Collections.Generic;\nstatic class StandIn {{\n  static void Main(string[] arguments) {{\n    var lines = new List<string>();\n    lines.Add(\"path=\" + System.Diagnostics.Process.GetCurrentProcess().MainModule.FileName);\n    foreach (var argument in arguments) lines.Add(\"arg=\" + argument);\n    System.IO.File.WriteAllLines(@\"{verbatim}.part\", lines);\n    System.IO.File.Move(@\"{verbatim}.part\", @\"{verbatim}\");\n  }}\n}}\n"
        ),
    )
    .map_err(|error| error.to_string())?;
    let built = std::process::Command::new(&compiler)
        .arg("/nologo")
        .arg("/target:winexe")
        .arg(format!("/out:{}", program.display()))
        .arg(&source)
        .output()
        .map_err(|error| format!("{}: {error}", compiler.display()))?;
    if !built.status.success() {
        return Err(format!(
            "{}: {}",
            compiler.display(),
            String::from_utf8_lossy(&built.stdout)
        ));
    }
    std::fs::read(&program).map_err(|error| error.to_string())
}

/// Runs one update check as Check Now starts it, and waits until it has
/// finished.
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
const ENGINE_PAGES: [(&str, &str, &str); 4] = [
    ("tineye", "/tineye.com/", "/tineye.com/search/abc123"),
    ("saucenao", "/saucenao.com/", "/saucenao.com/search.php"),
    (
        "yandex",
        "/yandex.com/images/",
        "/yandex.com/images/search?rpt=imageview&cbir_id=42",
    ),
    (
        "bing",
        "/www.bing.com/images",
        "/www.bing.com/search?view=detailv2&iss=sbiupload&bcid=XYZ",
    ),
];

/// The page the server gives for an engine. Its upload control is where the
/// Electron app's script looks for it, behind a button for the engines that
/// hide it behind one. An upload leads to the results address.
fn engine_page(key: &str, result: &str) -> String {
    let control = match key {
        "tineye" => r#"<div id="upload-box"><input type="file" id="f"></div>"#.to_owned(),
        "saucenao" => r#"<form id="searchForm" method="post" enctype="multipart/form-data" action="/saucenao.com/search.php"><input type="file" id="fileInput" name="file"></form>"#.to_owned(),
        "yandex" => r#"<button class="input__cbir-button" id="b">image</button>"#.to_owned(),
        _ => r#"<div id="sb_sbi">camera</div>"#.to_owned(),
    };
    let script = match key {
        "tineye" => format!(
            "document.getElementById('f').addEventListener('change', e => send(e.target, '{result}'));"
        ),
        "saucenao" => String::new(),
        "yandex" | "bing" => {
            let (button, id) = match key {
                "yandex" => ("b", ""),
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

fn serve(origin: &str, installer: Vec<u8>) -> std::io::Result<Server> {
    let address = origin.trim_start_matches("http://");
    let listener = TcpListener::bind(address)?;
    let server = Server {
        origin: origin.to_owned(),
        installer: Arc::new(installer),
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
