//! The video backend that plays a file with the `ffmpeg` program. It plays
//! the videos that the platform's own player does not play, which are
//! mostly WebM and Matroska. Picto ships with `ffmpeg` and `ffprobe` for
//! making thumbnails and waveforms: `scripts/download-ffmpeg.sh` downloads
//! them, and `core/src/media_processing/ffmpeg_path.rs` finds them for the
//! core.
//!
//! One `ffmpeg` process writes the video's frames as raw BGRA pixels at a
//! constant frame rate, and the frames are drawn as images. A second
//! process writes the sound as 32-bit float samples, which the audio engine
//! (`audio.rs`) plays. A frame is shown when the playback position of the
//! sound reaches the frame's time. A seek ends both processes and starts
//! new ones at the new position.
use super::audio::{Analysis, Engine, OpenSource, Source};
use super::backend::{Backend, Frame, Status};
use gpui::RenderImage;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

/// The sample rate at which `ffmpeg` is asked to write the sound. The audio
/// engine converts it to the sample rate of the output device.
const AUDIO_RATE: u32 = 48_000;
/// How many decoded frames may wait to be shown. The thread that reads the
/// frames from `ffmpeg` stops reading while this many are waiting.
const FRAMES_AHEAD: usize = 3;

/// The shortest time between two restarts of the `ffmpeg` processes. A seek
/// that comes sooner after a restart is kept, and it is carried out once
/// this time has passed. When several seeks come in that time, only the
/// last one is carried out.
const SEEK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// Returns the path of the program `name`, which is `ffmpeg` or `ffprobe`,
/// or `None` when it is not found. The search (`find_binary`) is made once
/// for each name and its result is kept. The search looks through folders
/// and `PATH`, and a drag of the progress bar starts processes up to ten
/// times a second.
pub fn binary(name: &str) -> Option<PathBuf> {
    static FOUND: Mutex<Vec<(String, Option<PathBuf>)>> = Mutex::new(Vec::new());
    let mut found = FOUND
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, path)) = found.iter().find(|(known, _)| known == name) {
        return path.clone();
    }
    let path = find_binary(name);
    found.push((name.to_owned(), path.clone()));
    path
}

/// Returns a command that runs `program`. On Windows the command is set to
/// open no console window (`CREATE_NO_WINDOW`).
fn command(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Searches for the program `name` and returns its path, or `None` when it
/// is not found. It looks in the folder that the environment variable
/// `PICTO_FFMPEG_DIR` names, then in `vendor/ffmpeg/` in the working
/// directory and in the seven folders above it, then in `vendor/ffmpeg/` in
/// the executable's folder and in the eight folders above it, and then in
/// the folders of `PATH`. On Windows `.exe` is added to the name. The
/// places are those of `core/src/media_processing/ffmpeg_path.rs`.
fn find_binary(name: &str) -> Option<PathBuf> {
    let name = if cfg!(target_os = "windows") {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    if let Ok(dir) = std::env::var("PICTO_FFMPEG_DIR") {
        let path = PathBuf::from(dir).join(&name);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.extend(cwd.ancestors().take(8).map(Path::to_path_buf));
    }
    if let Ok(exe) = std::env::current_exe() {
        roots.extend(exe.ancestors().skip(1).take(9).map(Path::to_path_buf));
    }
    for root in roots {
        let path = root.join("vendor").join("ffmpeg").join(&name);
        if path.is_file() {
            return Some(path);
        }
    }
    let separator = if cfg!(target_os = "windows") {
        ';'
    } else {
        ':'
    };
    std::env::var("PATH").ok().and_then(|paths| {
        paths
            .split(separator)
            .map(|dir| PathBuf::from(dir).join(&name))
            .find(|path| path.is_file())
    })
}

/// What `ffprobe` reports about a video file.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// The width of the picture as it is shown, in pixels: after the
    /// video's rotation is applied, and rounded down to an even number.
    pub width: u32,
    /// The height of the picture as it is shown, in pixels, rounded down to
    /// an even number.
    pub height: u32,
    /// The frame rate, in frames a second.
    pub fps: f64,
    /// The length in seconds, or 0 when `ffprobe` reports none.
    pub duration: f64,
    /// True when the file has an audio stream.
    pub audio: bool,
    /// The YCbCr matrix with which the picture is converted to RGB, by the
    /// name that `ffmpeg`'s scale filter has for it.
    pub matrix: &'static str,
}

/// Returns the name that `ffmpeg`'s scale filter has for the matrix of the
/// `color_space` that `ffprobe` reports for a video stream. A stream that
/// names no colour space, or one that is not listed here, is converted with
/// BT.709, as web browsers convert it. The scale filter's own default is
/// BT.601, which would draw the reds and blues of a BT.709 video more
/// saturated than they are.
pub fn color_matrix(color_space: Option<&str>) -> &'static str {
    match color_space {
        Some("bt470bg" | "smpte170m" | "bt601") => "bt601",
        Some("bt2020nc" | "bt2020c") => "bt2020",
        Some("smpte240m") => "smpte240m",
        Some("fcc") => "fcc",
        _ => "bt709",
    }
}

/// Reads the JSON that `ffprobe -of json` writes and returns what the
/// player needs from it. The frame rate is the average frame rate of the
/// video stream, or its base frame rate when the average is missing. It is
/// 30 when neither is given or the rate is above 240. The width and the
/// height are swapped when the stream is rotated by 90° or 270°. Returns
/// an error when the JSON cannot be read, when the file has no video
/// stream, or when the stream has no picture size.
pub fn parse_probe(json: &str) -> Result<Info, String> {
    let probe: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("ffprobe: {error}"))?;
    let streams = probe["streams"].as_array().cloned().unwrap_or_default();
    let video = streams
        .iter()
        .find(|stream| stream["codec_type"] == "video")
        .ok_or("The file has no video stream")?;
    let ratio = |value: &serde_json::Value| -> Option<f64> {
        let text = value.as_str()?;
        let (numerator, denominator) = text.split_once('/').unwrap_or((text, "1"));
        let (numerator, denominator): (f64, f64) =
            (numerator.parse().ok()?, denominator.parse().ok()?);
        (denominator > 0. && numerator > 0.).then(|| numerator / denominator)
    };
    let fps = ratio(&video["avg_frame_rate"])
        .or_else(|| ratio(&video["r_frame_rate"]))
        .filter(|fps| *fps <= 240.)
        .unwrap_or(30.);
    let (mut width, mut height) = (
        video["width"].as_u64().unwrap_or(0) as u32,
        video["height"].as_u64().unwrap_or(0) as u32,
    );
    if width == 0 || height == 0 {
        return Err("The video has no picture size".into());
    }
    let rotation = video["side_data_list"]
        .as_array()
        .and_then(|list| list.iter().find_map(|data| data["rotation"].as_f64()))
        .or_else(|| {
            video["tags"]["rotate"]
                .as_str()
                .and_then(|rotate| rotate.parse().ok())
        })
        .unwrap_or(0.);
    if (rotation.abs() as i64 / 90) % 2 == 1 {
        std::mem::swap(&mut width, &mut height);
    }
    let seconds = |value: &serde_json::Value| {
        value
            .as_str()
            .and_then(|text| text.parse::<f64>().ok())
            .filter(|seconds| seconds.is_finite() && *seconds > 0.)
    };
    let duration = seconds(&probe["format"]["duration"])
        .or_else(|| seconds(&video["duration"]))
        .unwrap_or(0.);
    Ok(Info {
        // The width and the height are rounded down to even numbers, so
        // that the scale filter does not pad a video whose colour is
        // subsampled 4:2:0.
        width: width & !1,
        height: height & !1,
        fps,
        duration,
        audio: streams.iter().any(|stream| stream["codec_type"] == "audio"),
        matrix: color_matrix(video["color_space"].as_str()),
    })
}

/// Runs `ffprobe` on the file `path` and returns what it reports. Returns
/// an error when `ffprobe` is not found, cannot be run or fails. It writes
/// to the log how long `ffprobe` ran, so that a user's log shows whether
/// starting the program is what makes a video slow to open.
fn probe(path: &Path) -> Result<Info, String> {
    let ffprobe = binary("ffprobe").ok_or("Picto's bundled ffprobe was not found")?;
    let started = Instant::now();
    let output = command(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("ffprobe: {error}"))?;
    log::info!(
        target: "timing.ffprobe",
        "ffprobe ran for {} ms on {}",
        started.elapsed().as_millis(),
        path.display()
    );
    if !output.status.success() {
        return Err(format!(
            "ffprobe: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_probe(&String::from_utf8_lossy(&output.stdout))
}

/// A child process that is killed when this value is dropped. A new thread
/// then waits for the process to end, so that the thread that dropped the
/// value does not wait.
struct Process(Option<Child>);

impl Process {
    /// Starts `ffmpeg` with the arguments `arguments` before the input file
    /// `path` and the arguments `after` behind it. Returns the process and
    /// its standard output.
    fn spawn(
        arguments: &[String],
        path: &Path,
        after: &[String],
    ) -> Result<(Self, ChildStdout), String> {
        let ffmpeg = binary("ffmpeg").ok_or("Picto's bundled ffmpeg was not found")?;
        let mut child = command(&ffmpeg)
            .args(["-v", "error", "-nostdin"])
            .args(arguments)
            .arg("-i")
            .arg(path)
            .args(after)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("ffmpeg: {error}"))?;
        let stdout = child.stdout.take().ok_or("ffmpeg has no output")?;
        Ok((Self(Some(child)), stdout))
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

/// The frames of the video from one position on. An `ffmpeg` process writes
/// them, one for each `1 / fps` seconds, and a thread reads them.
struct VideoStream {
    /// The `ffmpeg` process. It is kept so that it is killed when the
    /// stream is dropped.
    _process: Process,
    /// The frames that the thread has read, each with its time in seconds.
    frames: mpsc::Receiver<(f64, Arc<RenderImage>)>,
    /// The next frame. It has been taken from `frames`, but its time has
    /// not come yet.
    pending: Option<(f64, Arc<RenderImage>)>,
    /// True once the thread that reads the frames has ended and all its
    /// frames have been taken from `frames`.
    done: bool,
}

impl VideoStream {
    /// Starts an `ffmpeg` process that decodes the first video stream of
    /// `path` from `start` seconds on, with the frame rate, the size and
    /// the colour matrix of `info`, and a thread that reads its frames.
    fn start(path: &Path, info: &Info, start: f64) -> Result<Self, String> {
        let (process, mut stdout) = Process::spawn(
            &["-ss".into(), format!("{start:.3}")],
            path,
            &[
                "-map".into(),
                "0:v:0".into(),
                "-an".into(),
                "-sn".into(),
                "-dn".into(),
                "-vf".into(),
                format!(
                    "fps={:.6},scale={}:{}:flags=bicubic:in_color_matrix={},format=bgra",
                    info.fps, info.width, info.height, info.matrix
                ),
                "-f".into(),
                "rawvideo".into(),
                "-".into(),
            ],
        )?;
        let (sender, frames) = mpsc::sync_channel(FRAMES_AHEAD);
        let (width, height, fps) = (info.width, info.height, info.fps);
        std::thread::Builder::new()
            .name("picto-video-frames".into())
            .spawn(move || {
                let size = width as usize * height as usize * 4;
                for index in 0u64.. {
                    let mut pixels = vec![0u8; size];
                    if stdout.read_exact(&mut pixels).is_err() {
                        return;
                    }
                    let Some(image) = image::RgbaImage::from_raw(width, height, pixels) else {
                        return;
                    };
                    let image = Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
                        image
                    )]));
                    let time = start + index as f64 / fps;
                    if sender.send((time, image)).is_err() {
                        return;
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            _process: process,
            frames,
            pending: None,
            done: false,
        })
    }

    /// Returns the frame to show at the playback position `clock`: the
    /// latest frame whose time is not after `clock`. Earlier frames that
    /// have arrived are skipped. Returns `None` when no such frame has
    /// arrived since the last call. When `first` is true, the next frame is
    /// taken whatever its time, and later frames only if their time has
    /// come.
    fn due(&mut self, clock: f64, first: bool) -> Option<Arc<RenderImage>> {
        let mut shown = None;
        loop {
            if self.pending.is_none() {
                match self.frames.try_recv() {
                    Ok(frame) => self.pending = Some(frame),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.done = true;
                        break;
                    }
                }
            }
            match &self.pending {
                Some((time, _)) if *time <= clock || (first && shown.is_none()) => {
                    shown = self.pending.take().map(|(_, image)| image);
                }
                _ => break,
            }
        }
        shown
    }
}

/// The sound of the video as a source for the audio engine. An `ffmpeg`
/// process writes the samples. A seek ends the process and starts a new one
/// at the new position.
struct AudioStream {
    path: PathBuf,
    duration: Option<f64>,
    process: Option<(Process, ChildStdout)>,
}

impl AudioStream {
    /// Starts an `ffmpeg` process that writes the first audio stream of
    /// `path` from `start` seconds on, as stereo 32-bit float samples at
    /// 48,000 Hz.
    fn start(path: &Path, start: f64) -> Result<(Process, ChildStdout), String> {
        Process::spawn(
            &["-ss".into(), format!("{start:.3}")],
            path,
            &[
                "-map".into(),
                "0:a:0".into(),
                "-vn".into(),
                "-sn".into(),
                "-ac".into(),
                "2".into(),
                "-ar".into(),
                AUDIO_RATE.to_string(),
                "-f".into(),
                "f32le".into(),
                "-".into(),
            ],
        )
    }
}

impl Source for AudioStream {
    fn spec(&self) -> (u32, usize) {
        (AUDIO_RATE, 2)
    }

    fn duration(&self) -> Option<f64> {
        self.duration
    }

    fn seek(&mut self, seconds: f64) -> Result<(), String> {
        self.process = None;
        self.process = Some(Self::start(&self.path, seconds)?);
        Ok(())
    }

    fn next_chunk(&mut self) -> Option<Result<Vec<f32>, String>> {
        let (_, stdout) = self.process.as_mut()?;
        let mut bytes = vec![0u8; 4096 * 8];
        let mut filled = 0;
        while filled < bytes.len() {
            match stdout.read(&mut bytes[filled..]) {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Some(Err(error.to_string())),
            }
        }
        if filled < 8 {
            return None;
        }
        let samples = bytes[..filled / 8 * 8]
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .collect();
        Some(Ok(samples))
    }
}

/// The playback position of a video while it has no sound whose position it
/// can follow. It is computed from the time that has passed.
#[derive(Clone, Copy)]
struct Clock {
    /// The position, in seconds, at the moment `since`.
    base: f64,
    /// The moment from which the position advances, or `None` while
    /// playback is paused.
    since: Option<Instant>,
    /// The playback rate.
    rate: f64,
}

impl Clock {
    /// Returns the position now, in seconds.
    fn now(&self) -> f64 {
        self.base
            + self
                .since
                .map_or(0., |since| since.elapsed().as_secs_f64() * self.rate)
    }

    /// Sets the position to `position`. It advances from now on when
    /// `running` is true, and stays there when it is false.
    fn set(&mut self, position: f64, running: bool) {
        self.base = position;
        self.since = running.then(Instant::now);
    }
}

/// Returns true when the `ffmpeg` processes may be restarted for a seek.
/// `since` is the time since the last restart, or `None` when there has
/// been no restart. It must be at least `SEEK_INTERVAL`.
fn seek_due(since: Option<std::time::Duration>) -> bool {
    since.is_none_or(|since| since >= SEEK_INTERVAL)
}

/// The backend that plays a video with the `ffmpeg` program that Picto
/// ships with.
pub struct FfmpegPlayer {
    path: PathBuf,
    /// Where the thread that runs `ffprobe` puts its result.
    probe: Arc<Mutex<Option<Result<Info, String>>>>,
    /// What `ffprobe` reported, once `ready` has taken the result.
    info: Option<Info>,
    error: Option<String>,
    /// The stream of frames, or `None` before the file has been probed.
    video: Option<VideoStream>,
    /// The audio engine, or `None` when the file has no sound or has not
    /// been probed yet.
    audio: Option<Engine>,
    clock: Clock,
    playing: bool,
    /// The position of the last seek, in seconds. The sound and the frames
    /// start there when the file has been probed.
    target: f64,
    /// The volume that is given to the audio engine. It is 0 while the
    /// player is muted.
    volume: f32,
    rate: f32,
    /// True from the start of a stream of frames until its first frame has
    /// been taken. That frame is shown whatever its time.
    awaiting_first: bool,
    /// The position of a seek for which the processes have not been
    /// restarted yet, or `None` when there is no such seek.
    pending_seek: Option<f64>,
    /// The moment the processes were last started or restarted, or `None`
    /// before the first time.
    last_restart: Option<Instant>,
}

impl FfmpegPlayer {
    /// Makes a player for the file `path` and starts a thread that runs
    /// `ffprobe` on it. Returns an error when `ffmpeg` is not found. The
    /// sound and the frames are started later, by `ready`, when `ffprobe`
    /// has finished.
    pub fn open(path: &Path) -> Result<Self, String> {
        if binary("ffmpeg").is_none() {
            return Err("Picto's bundled ffmpeg was not found".into());
        }
        let result = Arc::new(Mutex::new(None));
        let (slot, file) = (result.clone(), path.to_path_buf());
        std::thread::Builder::new()
            .name("picto-ffprobe".into())
            .spawn(move || *slot.lock().unwrap() = Some(probe(&file)))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            path: path.to_path_buf(),
            probe: result,
            info: None,
            error: None,
            video: None,
            audio: None,
            clock: Clock {
                base: 0.,
                since: None,
                rate: 1.,
            },
            playing: false,
            target: 0.,
            volume: 0.,
            rate: 1.,
            awaiting_first: true,
            pending_seek: None,
            last_restart: None,
        })
    }

    /// Returns true when the file has been probed. The first time the
    /// result of `ffprobe` is there, it starts the audio engine, if the
    /// file has sound, and the stream of frames, both at the position of
    /// the last seek. When `ffprobe` failed, it records the error and
    /// returns false.
    fn ready(&mut self) -> bool {
        if self.info.is_some() {
            return true;
        }
        if self.error.is_some() {
            return false;
        }
        let Some(result) = self.probe.lock().unwrap().take() else {
            return false;
        };
        match result {
            Ok(info) => {
                if info.audio {
                    let (path, duration) = (self.path.clone(), Some(info.duration));
                    let open: OpenSource = Box::new(move |start| {
                        Ok(Box::new(AudioStream {
                            path: path.clone(),
                            duration: duration.filter(|duration| *duration > 0.),
                            process: Some(AudioStream::start(&path, start)?),
                        }) as Box<dyn Source>)
                    });
                    let engine = Engine::start(open, self.target, self.rate, !self.playing);
                    engine.set_volume(self.volume);
                    self.audio = Some(engine);
                }
                self.info = Some(info);
                // The sound and the frames both start at the position of
                // the last seek, so no seek is left to carry out.
                self.pending_seek = None;
                self.last_restart = Some(Instant::now());
                self.restart_video(self.target);
                true
            }
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    /// Carries out the seek that is waiting, if there is one: it restarts
    /// the sound and the frames at the seek's position. It does nothing
    /// when the processes were restarted less than 100 ms
    /// ([`SEEK_INTERVAL`]) ago. The seek then stays waiting, and a later
    /// call carries it out. `status` calls this function each time the
    /// player is drawn. So a drag of the progress bar restarts the
    /// processes at most ten times a second, and the last position of the
    /// drag is always carried out.
    fn apply_seek(&mut self) {
        let Some(seconds) = self.pending_seek else {
            return;
        };
        if !seek_due(self.last_restart.map(|at| at.elapsed())) {
            return;
        }
        self.pending_seek = None;
        self.last_restart = Some(Instant::now());
        if let Some(audio) = &self.audio {
            audio.seek(seconds);
        }
        if self.info.is_some() {
            self.restart_video(seconds);
        }
    }

    /// Ends the stream of frames and starts a new one at `position`. An
    /// error in starting it is recorded.
    fn restart_video(&mut self, position: f64) {
        let Some(info) = &self.info else {
            return;
        };
        self.video = None;
        match VideoStream::start(&self.path, info, position) {
            Ok(stream) => self.video = Some(stream),
            Err(error) => self.error = Some(error),
        }
        self.awaiting_first = true;
    }

    /// Returns the playback position in seconds. It is the position of the
    /// audio engine while the engine's output device is open and has no
    /// error, and the position of `clock` otherwise. It is not more than
    /// the length of the video.
    fn position(&self) -> f64 {
        let position = match &self.audio {
            Some(audio) if audio.running() => audio.position(),
            _ => self.clock.now(),
        };
        match &self.info {
            Some(info) if info.duration > 0. => position.min(info.duration),
            _ => position,
        }
    }

    /// Returns true when the video has reached its end: the last frame has
    /// been shown, and the audio engine has played the sound to its end.
    /// While the audio engine is not running, the position of `clock` must
    /// have reached the length of the video in its place.
    fn ended(&self) -> bool {
        let Some(info) = &self.info else {
            return false;
        };
        let pictures_done = self
            .video
            .as_ref()
            .is_none_or(|video| video.done && video.pending.is_none());
        let sound_done = match &self.audio {
            Some(audio) if audio.running() => audio.ended(),
            _ => info.duration <= 0. || self.clock.now() >= info.duration,
        };
        pictures_done && sound_done
    }
}

impl Backend for FfmpegPlayer {
    fn play(&mut self) {
        if self.playing {
            return;
        }
        self.playing = true;
        let position = self.position();
        self.clock.set(position, true);
        if let Some(audio) = &self.audio {
            audio.set_paused(false);
        }
    }

    fn pause(&mut self) {
        if !self.playing {
            return;
        }
        let position = self.position();
        self.playing = false;
        self.clock.set(position, false);
        if let Some(audio) = &self.audio {
            audio.set_paused(true);
        }
    }

    fn seek(&mut self, seconds: f64) {
        self.target = seconds;
        self.clock.set(seconds, self.playing);
        self.pending_seek = Some(seconds);
        self.apply_seek();
    }

    fn set_rate(&mut self, rate: f32) {
        let position = self.position();
        self.rate = rate;
        self.clock.rate = rate as f64;
        self.clock.set(position, self.playing);
        if let Some(audio) = &self.audio {
            audio.set_rate(rate);
        }
    }

    fn set_volume(&mut self, volume: f32, muted: bool) {
        self.volume = if muted { 0. } else { volume };
        if let Some(audio) = &self.audio {
            audio.set_volume(self.volume);
        }
    }

    fn status(&mut self) -> Status {
        self.ready();
        self.apply_seek();
        let ended = self.ended();
        if ended && self.playing {
            // The video has reached its end while playing, so playback
            // stops at the position it has reached.
            let position = self.position();
            self.playing = false;
            self.clock.set(position, false);
            if let Some(audio) = &self.audio {
                audio.set_paused(true);
            }
        }
        let info = self.info.clone();
        // Only the player's own error is reported. An error of the audio
        // engine is not: the video then plays without sound, and its
        // position is the one of `clock`.
        let error = self.error.clone();
        Status {
            position: self.position(),
            duration: info.as_ref().map_or(0., |info| info.duration),
            buffered: info.as_ref().map_or(0., |info| info.duration),
            playing: self.playing,
            ended,
            error,
            size: info.as_ref().map(|info| (info.width, info.height)),
            fps: info.as_ref().map(|info| info.fps),
        }
    }

    fn take_frame(&mut self) -> Option<Frame> {
        let clock = self.position();
        let first = self.awaiting_first;
        let image = self.video.as_mut()?.due(clock, first)?;
        self.awaiting_first = false;
        Some(Frame::Image(image))
    }

    fn analysis(&self) -> Option<Analysis> {
        self.audio.as_ref().map(Engine::analysis)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Backend, FfmpegPlayer, Frame, Info, SEEK_INTERVAL, color_matrix, parse_probe, seek_due,
    };

    #[test]
    fn a_scrub_restarts_the_decoders_at_most_every_interval() {
        assert!(seek_due(None));
        assert!(!seek_due(Some(SEEK_INTERVAL / 2)));
        assert!(seek_due(Some(SEEK_INTERVAL)));
    }
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    #[test]
    fn probe_output_is_read_with_rotation_and_fallbacks() {
        let json = r#"{"streams":[
            {"codec_type":"video","width":1920,"height":1081,"avg_frame_rate":"30000/1001",
             "side_data_list":[{"rotation":-90}]},
            {"codec_type":"audio"}],
            "format":{"duration":"10.051000"}}"#;
        let info = parse_probe(json).unwrap();
        assert_eq!(
            info,
            Info {
                width: 1080,
                height: 1920,
                fps: 30000. / 1001.,
                duration: 10.051,
                audio: true,
                matrix: "bt709",
            }
        );
        let silent = r#"{"streams":[{"codec_type":"video","width":640,"height":480,
            "avg_frame_rate":"0/0","r_frame_rate":"25/1"}],"format":{}}"#;
        let info = parse_probe(silent).unwrap();
        assert_eq!((info.fps, info.duration, info.audio), (25., 0., false));
        assert!(parse_probe(r#"{"streams":[{"codec_type":"audio"}]}"#).is_err());
    }

    #[test]
    fn untagged_video_converts_as_bt709() {
        assert_eq!(color_matrix(None), "bt709");
        assert_eq!(color_matrix(Some("unknown")), "bt709");
        assert_eq!(color_matrix(Some("smpte170m")), "bt601");
        assert_eq!(color_matrix(Some("bt470bg")), "bt601");
        assert_eq!(color_matrix(Some("bt2020nc")), "bt2020");
    }

    /// Plays the video named by the environment variable
    /// `PICTO_GPUI_TEST_WEBM` with `ffmpeg` until ten frames have arrived.
    /// It checks that each frame has the size that `ffprobe` reported, that
    /// the position does not go back from one frame to the next and is
    /// later at the tenth frame than at the first, and that after a seek to
    /// 5 s the position is within 0.2 s of 5 s.
    #[test]
    #[ignore = "needs a video in PICTO_GPUI_TEST_WEBM and the bundled ffmpeg"]
    fn real_webm_produces_frames() {
        let path = PathBuf::from(std::env::var("PICTO_GPUI_TEST_WEBM").unwrap());
        let mut player = FfmpegPlayer::open(&path).unwrap();
        player.set_volume(0., true);
        player.play();
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut times = Vec::new();
        while times.len() < 10 {
            assert!(
                Instant::now() < deadline,
                "no frames: {:?}",
                player.status()
            );
            let status = player.status();
            assert!(status.error.is_none(), "{:?}", status.error);
            if let Some(Frame::Image(image)) = player.take_frame() {
                let size = image.size(0);
                assert_eq!(
                    Some((size.width.0 as u32, size.height.0 as u32)),
                    status.size
                );
                times.push(status.position);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            times.windows(2).all(|pair| pair[1] >= pair[0]) && times[9] > times[0],
            "{times:?}"
        );
        player.seek(5.);
        let status = player.status();
        assert!((status.position - 5.).abs() < 0.2, "{status:?}");
        eprintln!("{}: {:?}, frames at {times:?}", path.display(), status);
    }
}
