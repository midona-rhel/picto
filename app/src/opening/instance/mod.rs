//! Keeps one running instance of the app for each library copy. A second
//! launch on a copy that an instance already has open sends its arguments to
//! that instance and then ends, and that instance brings its window to the
//! front. Launches on different copies run at the same time. A launch that
//! has files and no copy (a file manager that opens a pack with the app)
//! sends the files to the instance that was active most recently.
//!
//! Each instance holds an exclusive lock on the file `<key>.lock` for as
//! long as it runs. The file is in a folder that only the user can reach.
//! The instance also listens on a local socket (a named pipe on Windows)
//! whose name contains the same key. The key identifies the copy. The system
//! releases the lock when the process ends, so an instance that crashed
//! does not block the next launch.
//!
//! A launch and an instance exchange three lines. The instance sends
//! `picto-gpui-instance 1 <pid>`. The launch sends one line of JSON,
//! `{"cwd": …, "args": […]}`. The instance answers `ok` after it has taken
//! the arguments.
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
/// How long a launch keeps trying to reach an instance that holds the lock.
/// The instance may still be starting, or another launch may have held the
/// lock for a moment while it looked for running instances.
const REACH: Duration = Duration::from_secs(5);
/// How long one exchange may take before the launch stops waiting for it.
const EXCHANGE: Duration = Duration::from_secs(10);
/// The largest number of bytes that an instance reads from one request.
const REQUEST_LIMIT: u64 = 1 << 20;

/// What a launch sends to the instance: the launch's working folder and its
/// arguments.
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

/// The running instance of one copy. It holds the lock until it is dropped.
pub struct Instance {
    _lock: File,
    active: PathBuf,
    copy: PathBuf,
}

impl Instance {
    /// Records that this instance was active most recently, by writing the
    /// file `<key>.active`. A launch that has files and no copy sends them to
    /// the instance whose file was written last.
    pub fn mark_active(&self) {
        let _ = std::fs::write(&self.active, self.copy.to_string_lossy().as_bytes());
    }
}

/// The result of starting on a copy.
pub enum Started {
    /// This process is the copy's instance. Requests from later launches
    /// are passed to `deliver`.
    Primary(Instance),
    /// An instance already had the copy open, and it took the request.
    HandedOver,
}

/// Returns the folder that holds the lock files and the sockets. Only the
/// user can reach it.
pub fn folder() -> std::io::Result<PathBuf> {
    transport::folder()
}

/// Returns the copy's key: the FNV-1a hash of what identifies the copy on
/// disk. On Unix that is the device and the inode of its folder. On Windows
/// it is the canonical path in lower case.
pub fn key(copy: &Path) -> std::io::Result<String> {
    let identity = transport::identity(copy)?;
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in identity {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(format!("{hash:016x}"))
}

/// Makes this process the instance of `copy`, or sends `request` to the
/// instance that already has `copy` open.
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
                // If the listener cannot start, this instance still holds
                // the lock. A later launch on the copy then fails with an
                // error, and the copy is not opened a second time.
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

/// Sends `request` to the instance that was active most recently. A launch
/// that has files and no copy uses this. Returns the copy that the instance
/// has open.
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
        // If the lock can be taken, no running instance holds it.
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

/// Does one exchange with the instance that listens under `key`. Returns an
/// error when the exchange takes longer than [`EXCHANGE`].
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

/// Does the launch's part of an exchange: reads the greeting, allows the
/// instance to bring its window to the front (`allow`), sends the request
/// and waits for `ok`.
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

/// Does the instance's part of one exchange: sends the greeting, reads the
/// request, passes it to `deliver` and answers `ok`. Answers `refused` when
/// the request cannot be read.
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
    // Wait until the launch closes the connection. If the instance closed it
    // first, the system could discard the answer before the launch read it.
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

    /// Runs two instances in this process. The first takes the lock and
    /// listens. A second start on the same copy sends its request to the first
    /// and does not become an instance. A start on another copy becomes an
    /// instance of its own. A launch with no copy reaches the instance that
    /// was marked active last.
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
        // `hand_to_latest_in` picks the instance by the modification time
        // of its `.active` file, so the two times must differ.
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
