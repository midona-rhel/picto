//! The sign-in window checked end to end in the running app, on whatever
//! system it runs on (debug builds only): `PICTO_GPUI_LOGIN_CHECK=<folder>`
//! with `PICTO_GPUI_LOGIN_ORIGIN=http://127.0.0.1:<port>` and a copy open.
//!
//! The check serves a login of its own at that origin (the Danbooru
//! service's addresses move there, `logins::sites`): a sign-in page with a
//! password field, a form post that sets a session cookie and redirects,
//! and a page with "Log out" once signed in. Then, through [`Logins`] as
//! the Accounts dialog drives it, it signs in four times, each in a new
//! sign-in window on the system's web view:
//!
//! - the page cannot load (the server closes the connection unanswered):
//!   the login reports "Danbooru could not load (…)"; then Cancel;
//! - the page loads and the login waits for the user ("Log in with
//!   Danbooru to continue.", which needs the page's cookies and a script
//!   run in it); then Cancel, which closes the window;
//! - the same, then the window closed as its close button does: "Login
//!   window closed.";
//! - the page posts its form by itself: the session cookie is set, the
//!   success rule fires, the credential goes to the application through
//!   `auth.credentials.set` into the process-local store, the window
//!   closes; the login is read back through `auth.credentials.list` and
//!   the core's store, then removed with `auth.credentials.delete`.
//!
//! A foreground task runs every few milliseconds throughout, so one is
//! ready while a web view is being made (WebView2 runs the thread's
//! messages then). Pictures of the window are saved in the folder, with
//! `login-check.log`, whose last line is `result: pass` or `result: fail`;
//! then the app quits.
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

/// The cookie the local login sets (Danbooru's own name).
const SESSION_COOKIE: &str = "_danbooru2_session";
const SITE: &str = "danbooru";
const WAITING: &str = "Log in with Danbooru to continue.";

/// What the local server does with a request.
#[derive(Clone, Copy)]
#[repr(u8)]
enum Mode {
    /// Serves the sign-in page, which waits for the user.
    Hold = 0,
    /// Serves the sign-in page, which posts its form by itself.
    Submit = 1,
    /// Closes every connection without an answer.
    Drop = 2,
}

#[derive(Clone)]
struct Server {
    mode: Arc<AtomicU8>,
    /// The session cookie's value, once the form was posted.
    issued: Arc<Mutex<Option<String>>>,
}

/// Starts the check if `PICTO_GPUI_LOGIN_CHECK` names a folder.
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
    // Always ready: a foreground task that borrows the app every few
    // milliseconds.
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
    // Every state the login passes through, as the Accounts dialog is told.
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

    // A page that cannot load.
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

    // The page loads and waits for the user; Cancel.
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

    // The page loads; the user closes the window.
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

    // The page signs in by itself.
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
        // The values are the check's own, made up for this run.
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

/// Starts a Danbooru login as the Accounts dialog's Log In does.
fn begin(logins: &Entity<Logins>, report: &mut Report, cx: &mut AsyncApp) -> bool {
    let started = cx.update(|cx| logins.update(cx, |logins, cx| logins.start(SITE, None, cx)));
    report.check_detail("the login starts", started.is_ok(), &format!("{started:?}"))
}

/// Cancel, as the Accounts dialog's: the login is idle and its window gone.
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

/// The sign-in page is up and the login waits for the user, which takes
/// its cookies and a script run in the page.
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
    let answer = cx.update(|cx| session.read(cx).dispatch(command.to_owned(), args));
    answer
        .await
        .unwrap_or_else(|_| Err("the application stopped".to_owned()))
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

/// `login-check.log`, also written to standard error as it goes.
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
        // The states hold no secret (`AuthState`).
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

/// Serves the local login at `origin` on threads of its own.
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
        // No answer: the connection just closes.
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

/// The request's method, path, `Cookie` header and body.
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
