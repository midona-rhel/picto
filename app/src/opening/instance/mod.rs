//! One instance per library copy, as production keeps one per machine
//! (`requestSingleInstanceLock`, `second-instance`): a second launch on the
//! copy an instance already has open hands its arguments to that instance,
//! which brings its window forward, and ends. Launches on other copies run
//! side by side. A launch with files and no copy (a file manager opening a
//! pack with the app) hands them to the instance most recently active.
//!
//! Each instance holds an exclusive lock on `<key>.lock` in a folder only
//! the user can reach, for as long as it runs, and listens on a local
//! socket (a named pipe on Windows) named after the same key; the key is the
//! copy's identity. The system lets the lock go with the process, so a
//! crashed instance leaves nothing that blocks the next.
//!
//! The exchange: the instance greets with `picto-gpui-instance 1 <pid>`, the
//! launch sends one JSON line `{"cwd": …, "args": […]}`, and the instance
//! answers `ok` once it has taken them.
#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as transport;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as transport;

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const GREETING: &str = "picto-gpui-instance 1";
/// How long a launch keeps trying to reach an instance that holds the lock
/// (one still starting, or one whose lock a probe held for a moment).
const REACH: Duration = Duration::from_secs(5);
/// How long one exchange may take before the launch gives up on it.
const EXCHANGE: Duration = Duration::from_secs(10);
/// The most a request may hold.
const REQUEST_LIMIT: u64 = 1 << 20;

/// What a launch hands the instance: its working folder and arguments, as
/// `second-instance` receives them.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub cwd: PathBuf,
    pub args: Vec<String>,
}

impl Request {
    fn encode(&self) -> String {
        let value = serde_json::json!({
            "cwd": self.cwd.to_string_lossy(),
            "args": self.args,
        });
        format!("{value}\n")
    }

    fn decode(line: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        Some(Self {
            cwd: PathBuf::from(value["cwd"].as_str()?),
            args: value["args"]
                .as_array()?
                .iter()
                .map(|arg| arg.as_str().map(str::to_owned))
                .collect::<Option<_>>()?,
        })
    }
}

/// The running instance: holds the lock until it ends.
pub struct Instance {
    _lock: File,
    active: PathBuf,
    copy: PathBuf,
}

impl Instance {
    /// Notes this instance as the one most recently active, which a launch
    /// with files and no copy hands them to.
    pub fn mark_active(&self) {
        let _ = std::fs::write(&self.active, self.copy.to_string_lossy().as_bytes());
    }
}

/// What starting on a copy came to.
pub enum Started {
    /// This process is the copy's instance; requests from later launches
    /// arrive through `deliver`.
    Primary(Instance),
    /// An instance already had the copy open and took the request.
    HandedOver,
}

/// Where the locks and sockets are: a folder of the user's own.
pub fn folder() -> std::io::Result<PathBuf> {
    transport::folder()
}

/// The copy's key: FNV-1a of its identity on disk (device and inode on
/// Unix; the canonical path, case folded, on Windows).
pub fn key(copy: &Path) -> std::io::Result<String> {
    let identity = transport::identity(copy)?;
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in identity {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(format!("{hash:016x}"))
}

/// Becomes `copy`'s instance, or hands `request` to the one it has.
pub fn start(
    copy: &Path,
    request: &Request,
    deliver: impl Fn(Request) + Send + Sync + 'static,
) -> Result<Started, String> {
    let folder = folder().map_err(|error| format!("instance folder: {error}"))?;
    start_in(&folder, copy, request, deliver)
}

fn start_in(
    folder: &Path,
    copy: &Path,
    request: &Request,
    deliver: impl Fn(Request) + Send + Sync + 'static,
) -> Result<Started, String> {
    let key = key(copy).map_err(|error| format!("{}: {error}", copy.display()))?;
    let lock_path = folder.join(format!("{key}.lock"));
    let deadline = Instant::now() + REACH;
    let mut last_error: String;
    loop {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|error| format!("{}: {error}", lock_path.display()))?;
        match lock.try_lock() {
            Ok(()) => {
                // Without its listener the instance still holds the copy;
                // later launches on it are then refused rather than opened
                // twice.
                if let Err(error) = transport::serve(folder, &key, std::sync::Arc::new(deliver)) {
                    eprintln!("picto: later launches on this copy cannot reach it: {error}");
                }
                let instance = Instance {
                    _lock: lock,
                    active: folder.join(format!("{key}.active")),
                    copy: copy.to_path_buf(),
                };
                instance.mark_active();
                return Ok(Started::Primary(instance));
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                drop(lock);
                match send(folder, &key, request) {
                    Ok(()) => return Ok(Started::HandedOver),
                    Err(error) => last_error = error,
                }
            }
            Err(std::fs::TryLockError::Error(error)) => {
                return Err(format!("{}: {error}", lock_path.display()));
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "another instance has {} open and could not be reached: {last_error}",
                copy.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Hands `request` to the instance most recently active, for a launch
/// with files and no copy. Returns the copy that instance has open.
pub fn hand_to_latest(request: &Request) -> Result<PathBuf, String> {
    let folder = folder().map_err(|error| format!("instance folder: {error}"))?;
    hand_to_latest_in(&folder, request)
}

fn hand_to_latest_in(folder: &Path, request: &Request) -> Result<PathBuf, String> {
    let entries = std::fs::read_dir(folder).map_err(|error| error.to_string())?;
    let mut live = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("lock") {
            continue;
        }
        let Some(key) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Ok(lock) = OpenOptions::new().write(true).open(&path) else {
            continue;
        };
        // A lock that can be taken belongs to no running instance.
        if !matches!(lock.try_lock(), Err(std::fs::TryLockError::WouldBlock)) {
            continue;
        }
        let active = folder.join(format!("{key}.active"));
        let Ok(modified) = std::fs::metadata(&active).and_then(|meta| meta.modified()) else {
            continue;
        };
        let copy = std::fs::read_to_string(&active).unwrap_or_default();
        live.push((modified, key.to_owned(), PathBuf::from(copy)));
    }
    live.sort_by_key(|(modified, _, _)| *modified);
    let (_, key, copy) = live
        .pop()
        .ok_or_else(|| "no instance of the app is open".to_string())?;
    let deadline = Instant::now() + REACH;
    loop {
        match send(folder, &key, request) {
            Ok(()) => return Ok(copy),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// One exchange with the instance listening under `key`, given up after
/// [`EXCHANGE`].
fn send(folder: &Path, key: &str, request: &Request) -> Result<(), String> {
    let stream = transport::connect(folder, key).map_err(|error| error.to_string())?;
    let request = request.clone();
    let (done, outcome) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(ask(stream, &request, transport::allow_foreground));
    });
    outcome
        .recv_timeout(EXCHANGE)
        .map_err(|_| "the open instance did not answer in time".to_string())?
}

/// The launch's side: reads the greeting, lets the instance take the
/// foreground, sends the request and waits for `ok`.
fn ask<S: Read + Write>(stream: S, request: &Request, allow: fn(u32)) -> Result<(), String> {
    let mut reader = BufReader::new(stream);
    let mut greeting = String::new();
    reader
        .read_line(&mut greeting)
        .map_err(|error| error.to_string())?;
    let pid = greeting
        .trim_end()
        .strip_prefix(GREETING)
        .and_then(|rest| rest.trim().parse::<u32>().ok())
        .ok_or_else(|| format!("unexpected greeting {greeting:?}"))?;
    allow(pid);
    let stream = reader.get_mut();
    stream
        .write_all(request.encode().as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|error| error.to_string())?;
    let mut answer = String::new();
    reader
        .read_line(&mut answer)
        .map_err(|error| error.to_string())?;
    if answer.trim_end() == "ok" {
        Ok(())
    } else {
        Err(format!(
            "the open instance refused the request ({answer:?})"
        ))
    }
}

/// The instance's side of one exchange.
fn answer<S: Read + Write>(stream: S, deliver: &(impl Fn(Request) + ?Sized)) {
    let mut reader = BufReader::new(stream);
    let greeting = format!("{GREETING} {}\n", std::process::id());
    if reader
        .get_mut()
        .write_all(greeting.as_bytes())
        .and_then(|()| reader.get_mut().flush())
        .is_err()
    {
        return;
    }
    let mut line = String::new();
    if (&mut reader)
        .take(REQUEST_LIMIT)
        .read_line(&mut line)
        .is_err()
    {
        return;
    }
    let reply = match Request::decode(line.trim_end()) {
        Some(request) => {
            deliver(request);
            "ok\n"
        }
        None => "refused\n",
    };
    let _ = reader.get_mut().write_all(reply.as_bytes());
    let _ = reader.get_mut().flush();
    // Read until the launch hangs up, so the answer is never discarded with
    // the connection before it has been read.
    let mut rest = [0u8; 64];
    let _ = reader.get_mut().read(&mut rest);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_request_reads_back_as_written() {
        let request = Request {
            cwd: PathBuf::from("/work dir"),
            args: vec!["a\nb.picto-pack".into(), "ü.library".into()],
        };
        assert_eq!(Request::decode(request.encode().trim_end()), Some(request));
        assert_eq!(Request::decode("{\"cwd\": 1}"), None);
        assert_eq!(Request::decode("not json"), None);
    }

    #[test]
    fn the_key_is_the_copy_whatever_path_reaches_it() {
        let folder = tempfile::tempdir().expect("folder");
        let copy = folder.path().join("Copy.library");
        std::fs::create_dir(&copy).expect("copy");
        let other = folder.path().join("Other.library");
        std::fs::create_dir(&other).expect("other");
        let roundabout = copy.join("..").join("Copy.library");
        assert_eq!(key(&copy).unwrap(), key(&roundabout).unwrap());
        assert_ne!(key(&copy).unwrap(), key(&other).unwrap());
    }

    /// Two instances of the protocol in this process: the first takes the
    /// lock and listens; a second start on the same copy hands its request
    /// over and does not start; a start on another copy runs beside it; a
    /// launch with no copy reaches the one most recently active.
    #[test]
    fn a_second_start_on_the_same_copy_hands_over() {
        let scratch = tempfile::tempdir().expect("scratch");
        let folder = scratch.path().join("instances");
        std::fs::create_dir(&folder).expect("folder");
        let copy = scratch.path().join("Copy.library");
        let other = scratch.path().join("Other.library");
        std::fs::create_dir(&copy).expect("copy");
        std::fs::create_dir(&other).expect("other");
        let received = Arc::new(Mutex::new(Vec::new()));
        let into = received.clone();
        let request = Request {
            cwd: scratch.path().to_path_buf(),
            args: vec!["a.picto-pack".into()],
        };
        let first = start_in(&folder, &copy, &request, move |request| {
            into.lock().unwrap().push(("copy", request))
        })
        .expect("first start");
        let Started::Primary(first) = first else {
            panic!("the first start found an instance");
        };
        let second = start_in(&folder, &copy, &request, |_| panic!("second listener"))
            .expect("second start");
        assert!(matches!(second, Started::HandedOver));
        assert_eq!(
            received.lock().unwrap().clone(),
            vec![("copy", request.clone())]
        );

        let into = received.clone();
        let beside = start_in(&folder, &other, &request, move |request| {
            into.lock().unwrap().push(("other", request))
        })
        .expect("start on another copy");
        let Started::Primary(beside) = beside else {
            panic!("another copy's start found an instance");
        };
        // The modification times must differ for the order to show.
        std::thread::sleep(Duration::from_millis(20));
        first.mark_active();
        let handed = Request {
            cwd: scratch.path().to_path_buf(),
            args: vec!["b.picto-pack".into()],
        };
        assert_eq!(hand_to_latest_in(&folder, &handed).expect("handed"), copy);
        std::thread::sleep(Duration::from_millis(20));
        beside.mark_active();
        assert_eq!(hand_to_latest_in(&folder, &handed).expect("handed"), other);
        assert_eq!(
            received.lock().unwrap().clone(),
            vec![
                ("copy", request.clone()),
                ("copy", handed.clone()),
                ("other", handed.clone())
            ]
        );
        drop(first);
        drop(beside);
    }

    #[test]
    fn a_launch_without_an_instance_is_told_so() {
        let scratch = tempfile::tempdir().expect("scratch");
        let request = Request {
            cwd: scratch.path().to_path_buf(),
            args: vec![],
        };
        assert!(hand_to_latest_in(scratch.path(), &request).is_err());
    }

    #[test]
    fn the_exchange_refuses_a_stranger() {
        struct Fake(std::io::Cursor<Vec<u8>>, Vec<u8>);
        impl Read for Fake {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(buffer)
            }
        }
        impl Write for Fake {
            fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
                self.1.write(buffer)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let request = Request {
            cwd: PathBuf::from("/"),
            args: vec![],
        };
        let stranger = Fake(std::io::Cursor::new(b"hello\n".to_vec()), Vec::new());
        assert!(ask(stranger, &request, |_| {}).is_err());
        let refused = Fake(
            std::io::Cursor::new(format!("{GREETING} 7\nrefused\n").into_bytes()),
            Vec::new(),
        );
        assert!(ask(refused, &request, |pid| assert_eq!(pid, 7)).is_err());
    }
}
