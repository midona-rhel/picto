//! A check of the whole sign-in in the running app, with the real sign-in
//! window and the web view of the operating system it runs on. Only a debug
//! build holds it. It runs when the app is started with
//! `PICTO_GPUI_LOGIN_CHECK=<folder>` and
//! `PICTO_GPUI_LOGIN_ORIGIN=http://127.0.0.1:<port>`, and it starts once a
//! library is open.
//!
//! The check runs a small web server at that origin, to which the addresses
//! of the Danbooru service are moved (`logins::sites`). The server serves a
//! sign-in page with a password field, accepts the form's `POST`, sets a
//! session cookie and redirects, and serves a page with a "Log out" link to
//! a signed-in session. The check then starts four logins through
//! [`Logins`], the same way the Accounts dialog does. Each login opens a new
//! sign-in window:
//!
//! 1. The server closes the connection without a response, so the page
//!    cannot load. The login must report "Danbooru could not load (…)". The
//!    check then cancels the login.
//! 2. The page loads and the login waits for the user, with the message
//!    "Log in with Danbooru to continue.". The login reaches that message
//!    only if the page's cookies can be read and a script can be run in the
//!    page. The check then cancels the login, which must close the window.
//! 3. The page loads as in the second login, and the check closes the
//!    window the way its close button does. The login must report "Login
//!    window closed.".
//! 4. The page submits its form by itself. The session cookie is set, the
//!    adapter counts the user as signed in, and the credential is saved
//!    with `auth.credentials.set` in the store that the process keeps in
//!    memory. The window must close. The check reads the login back with
//!    `auth.credentials.list` and from the core's credential store, and
//!    removes it with `auth.credentials.delete`.
//!
//! For the whole run a foreground task wakes every 3 ms. WebView2 processes
//! the thread's messages while it creates a web view, so a task of the app
//! can run in the middle of the creation. The check makes sure that a task
//! is ready to run at that moment, so that it covers this case.
//!
//! The check saves pictures of the sign-in window in the folder, and the
//! file `login-check.log`, whose last line is `result: pass` or
//! `result: fail`. Then the app quits.
use super::{Logins, Status};
use crate::platform::login_window::LoginWindow;
use gpui::*;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The name of the cookie that the local server sets when the form is
/// submitted. It is the name of Danbooru's own session cookie.
const SESSION_COOKIE: &str = "_danbooru2_session";
const SITE: &str = "danbooru";
const WAITING: &str = "Log in with Danbooru to continue.";

/// What the local server does with the requests it receives.
#[derive(Clone, Copy)]
#[repr(u8)]
enum Mode {
    /// Serves a sign-in page that waits for the user to submit its form.
    Hold = 0,
    /// Serves a sign-in page that submits its form by itself after 400 ms.
    Submit = 1,
    /// Closes every connection without a response.
    Drop = 2,
}

#[derive(Clone)]
struct Server {
    mode: Arc<AtomicU8>,
    /// The value of the session cookie that the server set when the form
    /// was submitted. It is `None` until then.
    issued: Arc<Mutex<Option<String>>>,
}

/// Starts the check if `PICTO_GPUI_LOGIN_CHECK` names a folder. Does
/// nothing otherwise.
pub fn start(cx: &mut App) {
    let Some(out) = crate::seams::login_check() else {
        return;
    };
    let mut report = Report::new(out.clone());
    let Some(origin) = crate::seams::login_origin() else {
        report.line("PICTO_GPUI_LOGIN_ORIGIN must name a loopback http origin");
        report.finish(false);
        cx.quit();
        return;
    };
    let server = match serve(&origin) {
        Ok(server) => server,
        Err(error) => {
            report.line(&format!("the local login could not be served at {origin}: {error}"));
            report.finish(false);
            cx.quit();
            return;
        }
    };
    report.line(&format!(
        "system {} {}; local login at {origin}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    // This foreground task wakes every 3 ms and borrows the app, until the
    // check has finished. It is there so that a task is ready to run while
    // WebView2 creates a web view (see the module comment).
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
        let passed = run(&server, &mut report, &out, cx).await;
        report.finish(passed);
        cx.update(|cx| {
            cx.set_global(Finished);
            cx.quit();
        });
    })
    .detach();
}

struct Finished;
impl Global for Finished {}

async fn run(server: &Server, report: &mut Report, out: &Path, cx: &mut AsyncApp) -> bool {
    let started = Instant::now();
    let session = loop {
        let session = cx.update(|cx| {
            crate::libraries::host::shell(cx).map(|shell| shell.read(cx).session().clone())
        });
        if let Some(session) = session {
            break session;
        }
        if started.elapsed() > Duration::from_secs(300) {
            report.line("no library opened within 300 s");
            return false;
        }
        pause(cx, 200).await;
    };
    report.line(&format!("library open after {:?}", started.elapsed()));
    let logins = cx.update(|cx| Logins::global(&session, cx));
    // `seen` collects every state that the login passes through. These
    // are the states that the Accounts dialog would show.
    let seen: Rc<RefCell<Vec<super::AuthState>>> = Rc::default();
    let _watch = cx.update(|cx| {
        let seen = seen.clone();
        cx.observe(&logins, move |logins, cx| {
            let state = logins.read(cx).state().clone();
            let mut seen = seen.borrow_mut();
            if seen.last() != Some(&state) {
                seen.push(state);
            }
        })
    });
    let mut passed = true;

    // The first login: the page cannot load.
    server.set(Mode::Drop);
    seen.borrow_mut().clear();
    passed &= begin(&logins, report, cx);
    let failed = until(cx, 30, |_| {
        seen.borrow().iter().any(|state| {
            state.status == Status::Error
                && state
                    .message
                    .as_deref()
                    .is_some_and(|message| message.starts_with("Danbooru could not load ("))
        })
    })
    .await;
    passed &= report.check("a page that cannot load is reported", failed, &seen);
    pause(cx, 1500).await;
    capture(&logins, out, "load-failed", report, cx);
    passed &= cancel(&logins, report, cx).await;

    // The second login: the page loads and waits for the user, and the
    // login is then cancelled.
    server.set(Mode::Hold);
    seen.borrow_mut().clear();
    passed &= begin(&logins, report, cx);
    let waiting = until(cx, 30, |cx| waits(&logins, cx)).await;
    passed &= report.check(
        "the sign-in page loads and the login waits for the user",
        waiting,
        &seen,
    );
    pause(cx, 1000).await;
    capture(&logins, out, "sign-in", report, cx);
    passed &= cancel(&logins, report, cx).await;

    // The third login: the page loads, and the window is closed the way
    // its close button closes it.
    seen.borrow_mut().clear();
    passed &= begin(&logins, report, cx);
    let waiting = until(cx, 30, |cx| waits(&logins, cx)).await;
    passed &= report.check("the sign-in page loads again", waiting, &seen);
    cx.update(|cx| {
        logins.update(cx, |logins, cx| {
            if let Some(window) = logins.window() {
                window.close_as_user(cx);
            }
        })
    });
    let closed = until(cx, 15, |cx| {
        let state = logins.read(cx).state().clone();
        state.status == Status::Cancelled
            && state.message.as_deref() == Some("Login window closed.")
            && logins.read(cx).window().is_none()
            && LoginWindow::open_count() == 0
    })
    .await;
    passed &= report.check(
        "closing the window cancels the login (\"Login window closed.\")",
        closed,
        &seen,
    );

    // The fourth login: the page submits its form by itself.
    server.set(Mode::Submit);
    seen.borrow_mut().clear();
    passed &= begin(&logins, report, cx);
    let completed = until(cx, 60, |cx| {
        let state = logins.read(cx).state();
        state.status == Status::Completed
            && state.message.as_deref() == Some("Danbooru session captured.")
    })
    .await;
    passed &= report.check(
        "the session cookie is captured and saved (\"Danbooru session captured.\")",
        completed,
        &seen,
    );
    let gone = until(cx, 15, |cx| {
        logins.read(cx).window().is_none() && LoginWindow::open_count() == 0
    })
    .await;
    passed &= report.check("the sign-in window closed once saved", gone, &seen);
    let issued = server.issued.lock().ok().and_then(|issued| issued.clone());
    let listed = dispatch(&session, "auth.credentials.list", json!({}), cx).await;
    let row = listed
        .as_ref()
        .ok()
        .and_then(|listed| listed.as_array())
        .and_then(|rows| rows.iter().find(|row| row["site_id"] == SITE))
        .cloned();
    passed &= report.check_detail(
        "auth.credentials.list has the login",
        row.as_ref()
            .is_some_and(|row| row["credential_type"] == "cookies"),
        &format!("{row:?}"),
    );
    let stored = picto_core::credential_store::get_credential(SITE)
        .ok()
        .flatten()
        .and_then(|credential| credential.cookies)
        .and_then(|cookies| cookies.get(SESSION_COOKIE).cloned());
    passed &= report.check_detail(
        "the process-local store holds the cookie the page set",
        issued.is_some() && stored == issued,
        // The two cookie values may be written to the log: the local
        // server made them up for this run, and no site knows them.
        &format!("set {issued:?}, stored {stored:?}"),
    );
    let removed = dispatch(
        &session,
        "auth.credentials.delete",
        json!({ "site_id": SITE }),
        cx,
    )
    .await;
    let listed = dispatch(&session, "auth.credentials.list", json!({}), cx).await;
    let still = listed
        .as_ref()
        .ok()
        .and_then(|listed| listed.as_array())
        .is_some_and(|rows| rows.iter().any(|row| row["site_id"] == SITE));
    passed &= report.check_detail(
        "auth.credentials.delete removes it",
        removed.is_ok() && !still,
        &format!("{removed:?}"),
    );
    cx.update(|cx| logins.update(cx, |logins, cx| logins.cancel(cx)));
    passed
}

/// Starts a Danbooru login the way the Log In button of the Accounts
/// dialog does, and writes to the report whether it started.
fn begin(logins: &Entity<Logins>, report: &mut Report, cx: &mut AsyncApp) -> bool {
    let started = cx.update(|cx| logins.update(cx, |logins, cx| logins.start(SITE, None, cx)));
    report.check_detail("the login starts", started.is_ok(), &format!("{started:?}"))
}

/// Cancels the login the way the Cancel button of the Accounts dialog
/// does. Returns whether, within 15 s, the login is idle and its window is
/// closed.
async fn cancel(logins: &Entity<Logins>, report: &mut Report, cx: &mut AsyncApp) -> bool {
    cx.update(|cx| logins.update(cx, |logins, cx| logins.cancel(cx)));
    let idle = until(cx, 15, |cx| {
        logins.read(cx).state().status == Status::Idle
            && logins.read(cx).window().is_none()
            && LoginWindow::open_count() == 0
    })
    .await;
    report.check_detail("Cancel closes the window and the login is idle", idle, "")
}

/// Returns whether the sign-in page has loaded and the login waits for the
/// user. The login reaches that state only after it has read the page's
/// cookies and run a script in the page.
fn waits(logins: &Entity<Logins>, cx: &App) -> bool {
    let state = logins.read(cx).state();
    state.status == Status::Active
        && state.message.as_deref() == Some(WAITING)
        && state
            .current_url
            .as_deref()
            .is_some_and(|url| url.ends_with("/session/new"))
}

fn capture(logins: &Entity<Logins>, out: &Path, name: &str, report: &mut Report, cx: &mut AsyncApp) {
    let path = out.join(format!("{name}.png"));
    let saved = cx.update(|cx| {
        logins.update(cx, |logins, cx| {
            logins.window().map(|window| window.capture(&path, cx))
        })
    });
    report.line(&match saved {
        Some(Ok(())) => format!("picture: {}", path.display()),
        Some(Err(error)) => format!("picture {name}: not taken: {error}"),
        None => format!("picture {name}: no window"),
    });
}

async fn dispatch(
    session: &Entity<crate::session::Session>,
    command: &str,
    args: Value,
    cx: &mut AsyncApp,
) -> Result<Value, String> {
    let answer = cx.update(|cx| session.read(cx).call(command.to_owned(), args));
    answer.await
}

async fn until(cx: &mut AsyncApp, seconds: u64, mut done: impl FnMut(&App) -> bool) -> bool {
    let started = Instant::now();
    loop {
        if cx.update(|cx| done(cx)) {
            return true;
        }
        if started.elapsed() > Duration::from_secs(seconds) {
            return false;
        }
        pause(cx, 100).await;
    }
}

async fn pause(cx: &mut AsyncApp, milliseconds: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(milliseconds))
        .await;
}

/// The lines of `login-check.log`. Each line is also written to standard
/// error when it is added.
struct Report {
    out: PathBuf,
    lines: Vec<String>,
}

impl Report {
    fn new(out: PathBuf) -> Self {
        Self {
            out,
            lines: Vec::new(),
        }
    }

    fn line(&mut self, line: &str) {
        eprintln!("picto: login check: {line}");
        self.lines.push(line.to_owned());
    }

    fn check(&mut self, what: &str, ok: bool, seen: &RefCell<Vec<super::AuthState>>) -> bool {
        // The states may be written to the log: an `AuthState` never holds
        // a secret.
        let states: Vec<String> = seen
            .borrow()
            .iter()
            .map(|state| {
                format!(
                    "{:?} {:?} {:?}",
                    state.status,
                    state.message.as_deref().unwrap_or(""),
                    state.current_url.as_deref().unwrap_or("")
                )
            })
            .collect();
        self.check_detail(what, ok, &states.join(" -> "))
    }

    fn check_detail(&mut self, what: &str, ok: bool, detail: &str) -> bool {
        let verdict = if ok { "pass" } else { "FAIL" };
        if detail.is_empty() {
            self.line(&format!("{verdict}: {what}"));
        } else {
            self.line(&format!("{verdict}: {what} [{detail}]"));
        }
        ok
    }

    fn finish(&mut self, passed: bool) {
        self.line(if passed { "result: pass" } else { "result: fail" });
        let _ = std::fs::create_dir_all(&self.out);
        let _ = std::fs::write(
            self.out.join("login-check.log"),
            format!("{}\n", self.lines.join("\n")),
        );
    }
}

impl Server {
    fn set(&self, mode: Mode) {
        self.mode.store(mode as u8, Ordering::SeqCst);
    }
}

/// Starts the local server at `origin`. It accepts connections on one
/// thread and handles each connection on a thread of its own.
fn serve(origin: &str) -> Result<Server, String> {
    let url = url::Url::parse(origin).map_err(|error| error.to_string())?;
    let address = format!(
        "{}:{}",
        url.host_str().unwrap_or("127.0.0.1"),
        url.port_or_known_default().unwrap_or(80)
    );
    let listener = TcpListener::bind(&address).map_err(|error| error.to_string())?;
    let server = Server {
        mode: Arc::new(AtomicU8::new(Mode::Hold as u8)),
        issued: Arc::default(),
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
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Some((method, path, cookie, _body)) = read_request(&mut stream) else {
        return;
    };
    let mode = server.mode.load(Ordering::SeqCst);
    if mode == Mode::Drop as u8 {
        // The connection is closed without a response, so the page cannot
        // load.
        let _ = stream.shutdown(std::net::Shutdown::Both);
        return;
    }
    let path = path.split('?').next().unwrap_or_default().to_owned();
    let signed_in = cookie
        .split(';')
        .any(|pair| pair.trim().starts_with(&format!("{SESSION_COOKIE}=")));
    let response = match (method.as_str(), path.as_str()) {
        ("GET", "/session/new") => {
            let submit = if mode == Mode::Submit as u8 {
                "<script>setTimeout(function () { document.forms[0].submit(); }, 400);</script>"
            } else {
                ""
            };
            page(
                "200 OK",
                "",
                &format!(
                    "<title>Sign in</title><h1>Local sign-in</h1>\
                     <p>A page served by the app's own check; no site is contacted.</p>\
                     <form method=\"post\" action=\"/session\">\
                     <p><label>Name <input name=\"name\" value=\"check\"></label></p>\
                     <p><label>Password <input type=\"password\" name=\"password\" value=\"check\"></label></p>\
                     <p><button type=\"submit\">Sign in</button></p></form>{submit}"
                ),
            )
        }
        ("POST", "/session") => {
            let value = format!(
                "check-{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
            if let Ok(mut issued) = server.issued.lock() {
                *issued = Some(value.clone());
            }
            format!(
                "HTTP/1.1 303 See Other\r\nLocation: /\r\n\
                 Set-Cookie: {SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax\r\n\
                 Content-Length: 0\r\nConnection: close\r\n\r\n"
            )
        }
        ("GET", "/") if signed_in => page(
            "200 OK",
            "",
            "<title>Signed in</title><h1>Signed in</h1><p><a href=\"/logout\">Log out</a></p>",
        ),
        ("GET", "/") => page(
            "200 OK",
            "",
            "<title>Local</title><p><a href=\"/session/new\">Sign in</a></p>",
        ),
        _ => page("404 Not Found", "", "<title>Not found</title>"),
    };
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn page(status: &str, headers: &str, body: &str) -> String {
    let body = format!("<!doctype html><html><body>{body}</body></html>");
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n{headers}\
         Content-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    )
}

/// Reads one HTTP request and returns its method, its path, the value of
/// its `Cookie` header and its body. Returns `None` if the connection ends
/// before the headers are complete or the headers are longer than 64 KiB.
fn read_request(stream: &mut TcpStream) -> Option<(String, String, String, Vec<u8>)> {
    let mut data = Vec::new();
    let mut buffer = [0u8; 4096];
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
    let mut cookie = String::new();
    let mut length = 0usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "cookie" => cookie = value.trim().to_owned(),
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
    Some((method, path, cookie, body))
}
