//! OnlyFans' login in a browser of the user's, outside the app
//! (`electron/windows/externalOnlyFansAuth.mjs`): Chrome, Edge, Brave or
//! Chromium started on a new, empty profile in a temporary folder (never
//! the user's own profile, whose cookies are not read), with its DevTools
//! protocol on the loopback. The login is complete when a request of
//! OnlyFans' API has carried its `x-bc` signature and the profile holds the
//! `sess` and `auth_id` cookies; the browser is then closed and its profile
//! removed. It gives up after 15 minutes. Cancelling the login (which the
//! app does when it quits) closes the browser and removes the profile at
//! once.
//!
//! The DevTools protocol is spoken over a WebSocket of the loopback, with
//! the little of RFC 6455 a client needs (`websocket` below).
use super::Credential;
use futures::channel::oneshot;
use gpui::App;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead as _, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// `AUTH_TIMEOUT_MS`.
const TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// `TARGET_POLL_MS`.
const TARGET_POLL: Duration = Duration::from_millis(500);
const COOKIE_NAMES: [&str; 3] = ["sess", "auth_id", "auth_uid"];

/// The outside browser's login, closed when dropped.
pub struct ExternalLogin {
    shared: Arc<Shared>,
}

/// The browser and its profile, which whoever ends the login first (the
/// watch, or a cancel) closes and removes.
struct Shared {
    closed: AtomicBool,
    child: Mutex<Option<Child>>,
    profile: Mutex<Option<tempfile::TempDir>>,
}

impl Shared {
    /// Closes the browser and removes its profile, once.
    fn end(&self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(mut child) = self.child.lock().ok().and_then(|mut slot| slot.take()) {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(profile) = self.profile.lock().ok().and_then(|mut slot| slot.take()) {
            crate::platform::remove_when_released(profile, "the OnlyFans login's browser profile");
        }
    }
}

impl ExternalLogin {
    /// Ends the login: the browser is closed and its profile removed now.
    pub fn close(self) {
        self.shared.end();
    }
}

impl Drop for ExternalLogin {
    fn drop(&mut self) {
        self.shared.end();
    }
}

type Finished = oneshot::Receiver<Result<Credential, String>>;

/// `launchExternalOnlyFansAuth`: opens the browser, or says why it could
/// not, and hands back what its login captures.
pub fn launch(_: &mut App) -> (Result<ExternalLogin, String>, Finished) {
    let (sender, finished) = oneshot::channel();
    let launched = (|| {
        let executable = find_chromium(&candidates())
            .ok_or_else(|| "Browser login requires Chrome, Edge, Brave, or Chromium.".to_owned())?;
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .map_err(|error| error.to_string())?
            .port();
        let profile = tempfile::Builder::new()
            .prefix("picto-onlyfans-auth-")
            .tempdir()
            .map_err(|error| error.to_string())?;
        let start = match crate::seams::login_origin() {
            Some(origin) => format!("{origin}/"),
            None => "https://onlyfans.com/".to_owned(),
        };
        let child = Command::new(&executable)
            .args([
                format!("--remote-debugging-port={port}"),
                "--remote-debugging-address=127.0.0.1".to_owned(),
                format!("--user-data-dir={}", profile.path().display()),
                "--no-first-run".to_owned(),
                "--no-default-browser-check".to_owned(),
                "--disable-sync".to_owned(),
                start,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not open the external login browser: {error}"))?;
        Ok::<_, String>((child, port, profile))
    })();
    match launched {
        Ok((child, port, profile)) => {
            let shared = Arc::new(Shared {
                closed: AtomicBool::new(false),
                child: Mutex::new(Some(child)),
                profile: Mutex::new(Some(profile)),
            });
            let watching = shared.clone();
            std::thread::spawn(move || {
                let result = watch(port, &watching);
                let cancelled = watching.closed.load(Ordering::SeqCst);
                watching.end();
                if !cancelled {
                    let _ = sender.send(result);
                }
            });
            (Ok(ExternalLogin { shared }), finished)
        }
        Err(error) => (Err(error), finished),
    }
}

/// Where each system keeps the browsers production looks for.
fn candidates() -> Vec<PathBuf> {
    // i18n: file paths
    if cfg!(target_os = "macos") {
        [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    } else if cfg!(windows) {
        ["LOCALAPPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .flat_map(|root| {
                // i18n: folder names of the browsers' installations
                const BROWSERS: [[&str; 4]; 4] = [
                    // i18n: folder names
                    ["Google", "Chrome", "Application", "chrome.exe"],
                    // i18n: folder names
                    ["Microsoft", "Edge", "Application", "msedge.exe"],
                    // i18n: folder names
                    ["BraveSoftware", "Brave-Browser", "Application", "brave.exe"],
                    // i18n: folder names
                    ["Chromium", "Application", "chrome.exe", ""],
                ];
                BROWSERS.map(|parts| {
                    parts
                        .iter()
                        .filter(|part| !part.is_empty())
                        .fold(root.clone(), |path, part| path.join(part))
                })
            })
            .collect()
    } else {
        let commands = [
            "google-chrome-stable",
            "google-chrome",
            "microsoft-edge",
            "brave-browser",
            "chromium",
            "chromium-browser",
        ];
        let path = std::env::var_os("PATH").unwrap_or_default();
        commands
            .into_iter()
            .filter_map(|command| {
                std::env::split_paths(&path)
                    .map(|folder| folder.join(command))
                    .find(|candidate| candidate.is_file())
            })
            .collect()
    }
}

/// `findExternalChromium`: the first browser that is there.
fn find_chromium(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|path| path.is_file()).cloned()
}

/// Watches the browser until its login is captured, it is closed, the login
/// is cancelled or the time is up; then closes it.
fn watch(port: u16, shared: &Shared) -> Result<Credential, String> {
    let started = Instant::now();
    let captured: Arc<Mutex<Option<Credential>>> = Arc::default();
    let headers: Arc<Mutex<Option<BTreeMap<String, String>>>> = Arc::default();
    let mut attached = HashSet::new();
    let result = loop {
        if shared.closed.load(Ordering::SeqCst) {
            break Err("OnlyFans login was cancelled.".to_owned());
        }
        if let Some(credential) = captured.lock().ok().and_then(|mut slot| slot.take()) {
            break Ok(credential);
        }
        if started.elapsed() >= TIMEOUT {
            break Err("OnlyFans login timed out.".to_owned());
        }
        let exited = shared
            .child
            .lock()
            .ok()
            .and_then(|mut child| child.as_mut().map(|child| child.try_wait()));
        if !matches!(exited, Some(Ok(None))) {
            break Err("OnlyFans login was closed before authentication completed.".to_owned());
        }
        for target in read_targets(port) {
            if attached.insert(target.clone()) {
                let captured = captured.clone();
                let headers = headers.clone();
                std::thread::spawn(move || {
                    let _ = follow_target(&target, &headers, &captured);
                });
            }
        }
        std::thread::sleep(TARGET_POLL);
    };
    result
}

/// The pages' DevTools addresses (`/json/list`).
fn read_targets(port: u16) -> Vec<String> {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return Vec::new();
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let request =
        format!("GET /json/list HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return Vec::new();
    }
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    let body = answer
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or_default();
    page_targets(body)
}

/// The `webSocketDebuggerUrl` of every page in a `/json/list` answer.
pub fn page_targets(body: &str) -> Vec<String> {
    let start = body.find('[').unwrap_or(0);
    let Ok(Value::Array(targets)) = serde_json::from_str::<Value>(&body[start..]) else {
        return Vec::new();
    };
    targets
        .iter()
        .filter(|target| target["type"] == "page")
        .filter_map(|target| target["webSocketDebuggerUrl"].as_str().map(str::to_owned))
        .collect()
}

/// Follows one page: its requests for OnlyFans' API signature, and its
/// cookies after each.
fn follow_target(
    url: &str,
    headers: &Mutex<Option<BTreeMap<String, String>>>,
    captured: &Mutex<Option<Credential>>,
) -> Result<(), String> {
    let mut socket = websocket::Client::connect(url)?;
    socket.send(&json!({ "id": 1, "method": "Network.enable" }).to_string())?;
    socket.send(&json!({ "id": 2, "method": "Network.getAllCookies" }).to_string())?;
    let mut onlyfans_requests = HashSet::new();
    let mut next_id = 3;
    loop {
        let message: Value = serde_json::from_str(&socket.receive()?).unwrap_or(Value::Null);
        if let Some(cookies) = cookies_answer(&message) {
            let signature = headers.lock().ok().and_then(|slot| slot.clone());
            if let (Some(signature), true) = (
                signature,
                cookies.contains_key("sess") && cookies.contains_key("auth_id"),
            ) {
                if let Ok(mut slot) = captured.lock() {
                    *slot = Some(Credential {
                        site_category: "onlyfans".into(),
                        credential_type: "cookies".into(),
                        cookies: Some(cookies),
                        headers: Some(signature),
                        ..Credential::default()
                    });
                }
                return Ok(());
            }
            continue;
        }
        if let Some(signature) = api_signature(&message, &mut onlyfans_requests) {
            if let Ok(mut slot) = headers.lock() {
                *slot = Some(signature);
            }
            socket
                .send(&json!({ "id": next_id, "method": "Network.getAllCookies" }).to_string())?;
            next_id += 1;
        }
    }
}

/// OnlyFans' session cookies in a `Network.getAllCookies` answer.
pub fn cookies_answer(message: &Value) -> Option<BTreeMap<String, String>> {
    let cookies = message["result"]["cookies"].as_array()?;
    let local = crate::seams::login_origin()
        .and_then(|origin| url::Url::parse(&origin).ok())
        .and_then(|origin| origin.host_str().map(str::to_owned));
    Some(
        cookies
            .iter()
            .filter(|cookie| {
                let domain = cookie["domain"].as_str().unwrap_or_default().to_lowercase();
                let domain = domain.trim_start_matches('.');
                match local.as_deref() {
                    // Logins kept on local pages: only the local origin's.
                    Some(local) => local == domain,
                    None => domain == "onlyfans.com" || domain.ends_with(".onlyfans.com"),
                }
            })
            .filter_map(|cookie| {
                let name = cookie["name"].as_str()?;
                let value = cookie["value"].as_str().filter(|value| !value.is_empty())?;
                COOKIE_NAMES
                    .contains(&name)
                    .then(|| (name.to_owned(), value.to_owned()))
            })
            .collect(),
    )
}

/// The `x-bc` and `user-agent` an OnlyFans API request carried
/// (`Network.requestWillBeSent`, or its `ExtraInfo` with the headers the
/// browser added).
pub fn api_signature(
    message: &Value,
    onlyfans_requests: &mut HashSet<String>,
) -> Option<BTreeMap<String, String>> {
    let params = &message["params"];
    let headers = match message["method"].as_str()? {
        "Network.requestWillBeSent" => {
            let url = url::Url::parse(params["request"]["url"].as_str()?).ok()?;
            let ours = match crate::seams::login_origin() {
                Some(origin) => url.as_str().starts_with(&origin),
                None => url.host_str() == Some("onlyfans.com"),
            };
            if !ours || !url.path().starts_with("/api2/")
            {
                return None;
            }
            onlyfans_requests.insert(params["requestId"].as_str()?.to_owned());
            &params["request"]["headers"]
        }
        "Network.requestWillBeSentExtraInfo"
            if onlyfans_requests.contains(params["requestId"].as_str()?) =>
        {
            &params["headers"]
        }
        _ => return None,
    };
    let header = |name: &str| {
        headers.as_object()?.iter().find_map(|(key, value)| {
            (key.eq_ignore_ascii_case(name))
                .then(|| value.as_str().map(str::trim).unwrap_or_default().to_owned())
                .filter(|value| !value.is_empty())
        })
    };
    Some(BTreeMap::from([
        ("x-bc".to_owned(), header("x-bc")?),
        ("user-agent".to_owned(), header("user-agent")?),
    ]))
}

/// A WebSocket client of the loopback: the handshake, masked text frames
/// out, and frames in (fragments joined, pings answered).
pub mod websocket {
    use super::*;

    pub struct Client {
        reader: BufReader<TcpStream>,
        writer: TcpStream,
    }

    impl Client {
        /// Connects to a `ws://127.0.0.1:<port>/…` address.
        pub fn connect(address: &str) -> Result<Self, String> {
            let url = url::Url::parse(address).map_err(|error| error.to_string())?;
            let host = url.host_str().unwrap_or_default();
            if url.scheme() != "ws" || !(host == "127.0.0.1" || host == "localhost") {
                return Err("Could not inspect the external login browser.".into());
            }
            let port = url.port().unwrap_or(80);
            let stream = TcpStream::connect((host, port)).map_err(|error| error.to_string())?;
            let mut writer = stream.try_clone().map_err(|error| error.to_string())?;
            let key = {
                use base64::Engine as _;
                base64::engine::general_purpose::STANDARD.encode(uuid::Uuid::new_v4().as_bytes())
            };
            let path = match url.query() {
                Some(query) => format!("{}?{query}", url.path()),
                None => url.path().to_owned(),
            };
            write!(
                writer,
                "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
            )
            .map_err(|error| error.to_string())?;
            let mut reader = BufReader::new(stream);
            let mut status = String::new();
            reader
                .read_line(&mut status)
                .map_err(|error| error.to_string())?;
            if !status.contains(" 101 ") {
                return Err("Could not inspect the external login browser.".into());
            }
            loop {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                if line == "\r\n" || line.is_empty() {
                    break;
                }
            }
            Ok(Self { reader, writer })
        }

        pub fn send(&mut self, text: &str) -> Result<(), String> {
            let frame = encode(
                0x1,
                text.as_bytes(),
                *uuid::Uuid::new_v4()
                    .as_bytes()
                    .first_chunk()
                    .unwrap_or(&[0; 4]),
            );
            self.writer
                .write_all(&frame)
                .map_err(|error| error.to_string())
        }

        /// The next text message.
        pub fn receive(&mut self) -> Result<String, String> {
            let mut message = Vec::new();
            loop {
                let (fin, opcode, payload) = read_frame(&mut self.reader)?;
                match opcode {
                    0x8 => return Err("Browser inspection closed.".into()),
                    0x9 => {
                        let pong = encode(0xA, &payload, [0; 4]);
                        self.writer
                            .write_all(&pong)
                            .map_err(|error| error.to_string())?;
                    }
                    0xA => {}
                    _ => {
                        message.extend_from_slice(&payload);
                        if fin {
                            return String::from_utf8(message).map_err(|error| error.to_string());
                        }
                    }
                }
            }
        }
    }

    /// One frame from the client: final, masked.
    pub fn encode(opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
        let mut frame = vec![0x80 | opcode];
        let length = payload.len();
        if length < 126 {
            frame.push(0x80 | length as u8);
        } else if length <= u16::MAX as usize {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(length as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(length as u64).to_be_bytes());
        }
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        frame
    }

    /// One frame: whether it is final, its opcode and its (unmasked)
    /// payload.
    pub fn read_frame(reader: &mut impl Read) -> Result<(bool, u8, Vec<u8>), String> {
        let mut head = [0u8; 2];
        reader
            .read_exact(&mut head)
            .map_err(|error| error.to_string())?;
        let fin = head[0] & 0x80 != 0;
        let opcode = head[0] & 0x0F;
        let masked = head[1] & 0x80 != 0;
        let mut length = u64::from(head[1] & 0x7F);
        if length == 126 {
            let mut extended = [0u8; 2];
            reader
                .read_exact(&mut extended)
                .map_err(|error| error.to_string())?;
            length = u64::from(u16::from_be_bytes(extended));
        } else if length == 127 {
            let mut extended = [0u8; 8];
            reader
                .read_exact(&mut extended)
                .map_err(|error| error.to_string())?;
            length = u64::from_be_bytes(extended);
        }
        // No DevTools message comes near this; a larger length is a broken
        // stream.
        if length > 256 * 1024 * 1024 {
            return Err("Browser inspection closed.".into());
        }
        let mut mask = [0u8; 4];
        if masked {
            reader
                .read_exact(&mut mask)
                .map_err(|error| error.to_string())?;
        }
        let mut payload = vec![0u8; length as usize];
        reader
            .read_exact(&mut payload)
            .map_err(|error| error.to_string())?;
        if masked {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
        }
        Ok((fin, opcode, payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_page_targets_are_followed() {
        let body = r#"[{"type":"page","webSocketDebuggerUrl":"ws://127.0.0.1:9/devtools/page/A"},
                      {"type":"service_worker","webSocketDebuggerUrl":"ws://127.0.0.1:9/devtools/page/B"},
                      {"type":"page"}]"#;
        assert_eq!(page_targets(body), ["ws://127.0.0.1:9/devtools/page/A"]);
        assert!(page_targets("not json").is_empty());
    }

    #[test]
    fn the_signature_comes_from_an_api_request_or_its_extra_headers() {
        let mut requests = HashSet::new();
        let page = json!({ "method": "Network.requestWillBeSent", "params": {
            "requestId": "1", "request": { "url": "https://onlyfans.com/", "headers": { "X-BC": "s", "User-Agent": "UA" } } } });
        assert_eq!(api_signature(&page, &mut requests), None);
        let api = json!({ "method": "Network.requestWillBeSent", "params": {
            "requestId": "2", "request": { "url": "https://onlyfans.com/api2/v2/users/me", "headers": { "Accept": "*/*" } } } });
        assert_eq!(api_signature(&api, &mut requests), None);
        assert!(requests.contains("2"));
        let extra = json!({ "method": "Network.requestWillBeSentExtraInfo", "params": {
            "requestId": "2", "headers": { "x-bc": " signature ", "user-agent": "Chrome" } } });
        assert_eq!(
            api_signature(&extra, &mut requests),
            Some(BTreeMap::from([
                ("user-agent".into(), "Chrome".into()),
                ("x-bc".into(), "signature".into())
            ]))
        );
        let elsewhere = json!({ "method": "Network.requestWillBeSentExtraInfo", "params": {
            "requestId": "9", "headers": { "x-bc": "s", "user-agent": "UA" } } });
        assert_eq!(api_signature(&elsewhere, &mut requests), None);
    }

    #[test]
    fn only_onlyfans_session_cookies_are_kept() {
        let answer = json!({ "id": 2, "result": { "cookies": [
            { "name": "sess", "value": "session", "domain": ".onlyfans.com" },
            { "name": "auth_id", "value": "42", "domain": "onlyfans.com" },
            { "name": "auth_uid", "value": "", "domain": "onlyfans.com" },
            { "name": "sess", "value": "other", "domain": "notonlyfans.com" },
            { "name": "analytics", "value": "x", "domain": "onlyfans.com" },
        ] } });
        assert_eq!(
            cookies_answer(&answer),
            Some(BTreeMap::from([
                ("auth_id".into(), "42".into()),
                ("sess".into(), "session".into())
            ]))
        );
        assert_eq!(
            cookies_answer(&json!({ "method": "Network.dataReceived" })),
            None
        );
    }

    #[test]
    fn frames_are_masked_out_and_read_back_whatever_their_length() {
        for length in [0usize, 5, 125, 126, 65_535, 70_000] {
            let payload: Vec<u8> = (0..length).map(|index| (index % 251) as u8).collect();
            let frame = websocket::encode(0x1, &payload, [1, 2, 3, 4]);
            let (fin, opcode, read) =
                websocket::read_frame(&mut std::io::Cursor::new(frame)).unwrap();
            assert!(fin);
            assert_eq!(opcode, 0x1);
            assert_eq!(read, payload, "{length}");
        }
    }

    #[test]
    fn the_first_browser_present_is_used() {
        let folder = tempfile::tempdir().unwrap();
        let edge = folder.path().join("msedge");
        std::fs::write(&edge, b"").unwrap();
        let missing = folder.path().join("chrome");
        assert_eq!(find_chromium(&[missing.clone(), edge.clone()]), Some(edge));
        assert_eq!(find_chromium(&[missing]), None);
    }
}
