//! Files an X11 drag offers to save rather than put on the drag: the Direct
//! Save protocol (XDS, `XdndDirectSave0`), the X11 form of a file promise,
//! which web browsers use for a dragged image or link and archive managers
//! for a file inside an archive. The source names the file in the
//! `XdndDirectSave0` property of its window; on the drop the target puts a
//! `file:` address there, asks for the `XdndDirectSave0` target of
//! `XdndSelection`, and the source writes the file and answers `S` (saved),
//! `E` (an error) or `F` (it will not; the target may read the data itself,
//! which this side does not do).
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{PromisedFile, PromisedFiles};
use x11rb::{
    COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT,
    connection::Connection,
    protocol::{
        Event,
        xproto::{
            AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt as _, CreateWindowAux,
            EventMask, PropMode, Window, WindowClass,
        },
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use crate::linux::drop_data::DIRECT_SAVE;
/// How long the source may take to write the file (a browser may fetch it).
const ANSWER_WAIT: Duration = Duration::from_secs(120);

/// The name a source suggests, as a name in one folder: its last part, and
/// none when nothing usable is left.
fn file_name(suggested: &str) -> Option<String> {
    let name = suggested.rsplit('/').next().unwrap_or(suggested).trim();
    let name: String = name
        .chars()
        .filter(|character| *character != '\0')
        .collect();
    (!name.is_empty() && name != "." && name != "..").then_some(name)
}

/// `name` in `folder`, or `stem (2).ext` and on when that is taken.
fn unused_path(folder: &Path, name: &str) -> PathBuf {
    let path = folder.join(name);
    if !path.exists() {
        return path;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, Some(extension)),
        _ => (name, None),
    };
    (2..)
        .map(|number| {
            folder.join(match extension {
                Some(extension) => format!("{stem} ({number}).{extension}"),
                None => format!("{stem} ({number})"),
            })
        })
        .find(|path| !path.exists())
        .unwrap()
}

/// A folder no other drop of this process has used, inside `directory`.
fn drop_folder(directory: &Path) -> std::io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let folder = directory.join(format!(
        "promised-{now:x}-{:x}-{:x}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&folder)?;
    Ok(folder)
}

/// This machine's name, which a `file:` address for XDS carries.
fn host_name() -> String {
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer is writable for its length; the name is cut at its
    // nul (or the buffer's end).
    let result = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if result != 0 {
        return String::new();
    }
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    String::from_utf8_lossy(&buffer[..end]).into_owned()
}

fn atom(conn: &RustConnection, name: &str) -> Result<u32, String> {
    Ok(conn
        .intern_atom(false, name.as_bytes())
        .map_err(|error| error.to_string())?
        .reply()
        .map_err(|error| error.to_string())?
        .atom)
}

/// Asks the source of the drag being dropped (`source`, its window) to save
/// the file it offers into a folder made for this drop, on a thread and a
/// connection of its own, and then tells it the drop is finished on
/// `target`'s behalf (`XdndFinished`, which the caller does not send).
/// `time` is the drop's.
pub(crate) fn receive(display: &str, source: Window, target: Window, time: u32) -> PromisedFiles {
    let folder = drop_folder(&gpui::promised_files_directory());
    let (promised, sender) = PromisedFiles::new(
        folder
            .as_ref()
            .cloned()
            .unwrap_or_else(|_| gpui::promised_files_directory()),
        1,
    );
    let display = display.to_owned();
    std::thread::spawn(move || {
        let report: PromisedFile = match (folder, RustConnection::connect(Some(&display))) {
            (Err(error), _) => Err(error.to_string()),
            (_, Err(error)) => Err(error.to_string()),
            (Ok(folder), Ok((conn, screen))) => {
                let report = save(&conn, screen, source, time, &folder);
                let finished = finish(&conn, source, target, report.is_ok());
                match (report, finished) {
                    (Err(error), Err(also)) => Err(format!("{error}; {also}")),
                    (report, finished) => {
                        if let Err(error) = finished {
                            log::warn!("{error}");
                        }
                        report
                    }
                }
            }
        };
        if let Err(error) = &report {
            log::warn!("a drag's direct save failed: {error}");
        }
        let _ = sender.unbounded_send(report);
    });
    promised
}

/// The exchange: the file's name from the source, the address it is to be
/// saved at, the request and the source's answer.
fn save(
    conn: &RustConnection,
    screen: usize,
    source: Window,
    time: u32,
    folder: &Path,
) -> PromisedFile {
    let fail = |error: x11rb::errors::ConnectionError| error.to_string();
    let direct_save = atom(conn, DIRECT_SAVE)?;
    let text = atom(conn, "text/plain")?;
    let selection = atom(conn, "XdndSelection")?;
    let suggested = conn
        .get_property(false, source, direct_save, AtomEnum::ANY, 0, 1024)
        .map_err(fail)?
        .reply()
        .map_err(|error| error.to_string())?;
    let name = file_name(&String::from_utf8_lossy(&suggested.value))
        .ok_or("the source named no file to save")?;
    let path = unused_path(folder, &name);
    let mut address = url::Url::from_file_path(&path).map_err(|()| "not an absolute path")?;
    let _ = address.set_host(
        Some(&host_name())
            .filter(|host| !host.is_empty())
            .map(String::as_str),
    );
    conn.change_property8(
        PropMode::REPLACE,
        source,
        direct_save,
        text,
        address.as_str().as_bytes(),
    )
    .map_err(fail)?;
    // A window of this connection's own receives the answer.
    let requestor = conn.generate_id().map_err(|error| error.to_string())?;
    let root = conn.setup().roots.get(screen).ok_or("no screen")?.root;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        requestor,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        COPY_FROM_PARENT,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )
    .map_err(fail)?;
    conn.convert_selection(requestor, selection, direct_save, direct_save, time)
        .map_err(fail)?;
    conn.flush().map_err(fail)?;
    let answer = wait_for_answer(conn, requestor, direct_save);
    let _ = conn.destroy_window(requestor);
    let _ = conn.flush();
    let saved = match answer? {
        answer if answer.starts_with(b"S") => {
            std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file())
        }
        _ => false,
    };
    if saved {
        Ok(path)
    } else {
        // Whatever the source left there is not taken.
        let _ = std::fs::remove_file(&path);
        Err("the source did not save the file".into())
    }
}

/// The source's answer: the property it set on `requestor`.
fn wait_for_answer(
    conn: &RustConnection,
    requestor: Window,
    property: u32,
) -> Result<Vec<u8>, String> {
    let until = Instant::now() + ANSWER_WAIT;
    while Instant::now() < until {
        let event = conn.poll_for_event().map_err(|error| error.to_string())?;
        let Some(event) = event else {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        };
        if let Event::SelectionNotify(notify) = event
            && notify.requestor == requestor
        {
            if notify.property == u32::from(AtomEnum::NONE) {
                return Err("the source refused to save the file".into());
            }
            let reply = conn
                .get_property(true, requestor, property, AtomEnum::ANY, 0, 16)
                .map_err(|error| error.to_string())?
                .reply()
                .map_err(|error| error.to_string())?;
            return Ok(reply.value);
        }
    }
    Err("the source did not answer".into())
}

/// `XdndFinished` from `target` to `source`.
fn finish(
    conn: &RustConnection,
    source: Window,
    target: Window,
    accepted: bool,
) -> Result<(), String> {
    let finished = atom(conn, "XdndFinished")?;
    let copy = atom(conn, "XdndActionCopy")?;
    let message = ClientMessageEvent {
        response_type: x11rb::protocol::xproto::CLIENT_MESSAGE_EVENT,
        format: 32,
        sequence: 0,
        window: source,
        type_: finished,
        data: ClientMessageData::from([
            target,
            u32::from(accepted),
            if accepted { copy } else { 0 },
            0,
            0,
        ]),
    };
    conn.send_event(false, source, EventMask::NO_EVENT, message)
        .map_err(|error| format!("XdndFinished not sent: {error}"))?
        .check()
        .map_err(|error| format!("XdndFinished refused: {error:?}"))?;
    conn.flush().map_err(|error| error.to_string())
}

/// A source of a direct-save drag, for the application's test: a window of
/// a connection of its own that owns `XdndSelection`, names `name` in its
/// `XdndDirectSave0` property and, asked to save, writes `contents` (or,
/// with `fail`, half of them and answers `E`) at the address given. It
/// answers once, then waits for `XdndFinished`.
pub struct DirectSaveSource {
    /// The source's window.
    pub window: Window,
    answered: std::thread::JoinHandle<Result<bool, String>>,
}

impl DirectSaveSource {
    pub fn start(
        display: Option<&str>,
        name: &str,
        contents: Vec<u8>,
        fail: bool,
    ) -> Result<Self, String> {
        let (conn, screen) = RustConnection::connect(display).map_err(|error| error.to_string())?;
        let fail_with = |error: x11rb::errors::ConnectionError| error.to_string();
        let window = conn.generate_id().map_err(|error| error.to_string())?;
        let root = conn.setup().roots[screen].root;
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            COPY_FROM_PARENT,
            &CreateWindowAux::new(),
        )
        .map_err(fail_with)?;
        let direct_save = atom(&conn, DIRECT_SAVE)?;
        let text = atom(&conn, "text/plain")?;
        let selection = atom(&conn, "XdndSelection")?;
        let finished = atom(&conn, "XdndFinished")?;
        conn.change_property8(
            PropMode::REPLACE,
            window,
            direct_save,
            text,
            name.as_bytes(),
        )
        .map_err(fail_with)?;
        conn.set_selection_owner(window, selection, x11rb::CURRENT_TIME)
            .map_err(fail_with)?;
        conn.flush().map_err(fail_with)?;
        let answered = std::thread::spawn(move || -> Result<bool, String> {
            let until = Instant::now() + Duration::from_secs(30);
            let mut saved = None;
            // What the source saw, for a failure's message.
            let mut seen = Vec::new();
            while Instant::now() < until {
                let event = conn.poll_for_event().map_err(|error| error.to_string())?;
                let Some(event) = event else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                seen.push(format!("{event:?}"));
                match event {
                    Event::SelectionRequest(request) if request.target == direct_save => {
                        let address = conn
                            .get_property(false, window, direct_save, AtomEnum::ANY, 0, 4096)
                            .map_err(|error| error.to_string())?
                            .reply()
                            .map_err(|error| error.to_string())?;
                        let address = String::from_utf8_lossy(&address.value).into_owned();
                        seen.push(address.clone());
                        // The address names this machine, which the `url`
                        // crate does not take for a path; its path is the
                        // file's.
                        let path = url::Url::parse(&address).ok().and_then(|address| {
                            url::Url::parse(&format!("file://{}", address.path()))
                                .ok()?
                                .to_file_path()
                                .ok()
                        });
                        let wrote = path.is_some_and(|path| {
                            let bytes = if fail {
                                &contents[..contents.len() / 2]
                            } else {
                                &contents[..]
                            };
                            std::fs::write(path, bytes).is_ok()
                        }) && !fail;
                        let answer: &[u8] = if wrote { b"S" } else { b"E" };
                        conn.change_property8(
                            PropMode::REPLACE,
                            request.requestor,
                            request.property,
                            AtomEnum::STRING,
                            answer,
                        )
                        .map_err(fail_with)?;
                        let notify = x11rb::protocol::xproto::SelectionNotifyEvent {
                            response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
                            sequence: 0,
                            time: request.time,
                            requestor: request.requestor,
                            selection: request.selection,
                            target: request.target,
                            property: request.property,
                        };
                        conn.send_event(false, request.requestor, EventMask::NO_EVENT, notify)
                            .map_err(fail_with)?;
                        conn.flush().map_err(fail_with)?;
                        saved = Some(wrote);
                    }
                    Event::ClientMessage(message) if message.type_ == finished => {
                        return saved.ok_or_else(|| "finished before any request".to_owned());
                    }
                    _ => {}
                }
            }
            Err(format!("no XdndFinished; the source saw {seen:?}"))
        });
        Ok(Self { window, answered })
    }

    /// Whether the file was written, once the drop was finished.
    pub fn finished(self) -> Result<bool, String> {
        self.answered
            .join()
            .map_err(|_| "the source's thread panicked".to_owned())?
    }
}

/// [`receive`], for the application's test of a drop of its own making.
pub fn receive_direct_save(display: &str, source: Window, target: Window) -> PromisedFiles {
    receive(display, source, target, x11rb::CURRENT_TIME)
}
