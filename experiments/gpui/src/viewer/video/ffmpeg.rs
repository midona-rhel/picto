//! Video that the platform's player does not take (WebM and Matroska above
//! all), decoded by the `ffmpeg` binary Picto already bundles for thumbnails
//! and waveforms (`core/src/media_processing/ffmpeg_path.rs`, fetched by
//! `scripts/download-ffmpeg.sh`). One process writes raw BGRA frames at a
//! constant rate, which are uploaded as images; another writes the sound as
//! 32-bit float samples for the audio engine, whose clock the pictures
//! follow. Seeking restarts both at the new position.
use super::audio::{Analysis, Engine, OpenSource, Source};
use super::backend::{Backend, Frame, Status};
use gpui::RenderImage;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

/// The rate the sound is asked for; the engine fits it to the device.
const AUDIO_RATE: u32 = 48_000;
/// Decoded frames held ahead of the clock.
const FRAMES_AHEAD: usize = 3;

/// How often a scrub may restart the decoders: a seek that comes sooner
/// waits and is applied once this has passed (the last one wins).
const SEEK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// [`find_binary`], looked up once per name: a scrub starts processes
/// often, and the search walks folders and `PATH`.
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

/// A command for a bundled binary that opens no console window on Windows
/// (`CREATE_NO_WINDOW`, as the application core's own ffmpeg calls do).
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

/// Where the bundled binaries are, by the rules of `ffmpeg_path.rs`:
/// `PICTO_FFMPEG_DIR`, then `vendor/ffmpeg/` in the working directory or
/// above the executable, then `PATH`.
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

/// What `ffprobe` says about the file.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// The picture's size as shown: rotation applied.
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
    pub audio: bool,
    /// The YCbCr matrix the picture is converted to RGB with, as the
    /// scaler names it.
    pub matrix: &'static str,
}

/// The scaler's matrix for a stream's `color_space`. A stream that does not
/// say is taken as BT.709, as Chromium takes it (the scaler's own default is
/// BT.601, which draws reds and blues more saturated than production).
pub fn color_matrix(color_space: Option<&str>) -> &'static str {
    match color_space {
        Some("bt470bg" | "smpte170m" | "bt601") => "bt601",
        Some("bt2020nc" | "bt2020c") => "bt2020",
        Some("smpte240m") => "smpte240m",
        Some("fcc") => "fcc",
        _ => "bt709",
    }
}

/// Reads `ffprobe -of json` output.
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
        // Even sizes keep 4:2:0 sources from being padded by the scaler.
        width: width & !1,
        height: height & !1,
        fps,
        duration,
        audio: streams.iter().any(|stream| stream["codec_type"] == "audio"),
        matrix: color_matrix(video["color_space"].as_str()),
    })
}

fn probe(path: &Path) -> Result<Info, String> {
    let ffprobe = binary("ffprobe").ok_or("Picto's bundled ffprobe was not found")?;
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
    if !output.status.success() {
        return Err(format!(
            "ffprobe: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_probe(&String::from_utf8_lossy(&output.stdout))
}

/// A child process killed when dropped; the reaping happens off the caller's
/// thread.
struct Process(Option<Child>);

impl Process {
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

/// Pictures from `start`, one per `1 / fps` seconds.
struct VideoStream {
    _process: Process,
    frames: mpsc::Receiver<(f64, Arc<RenderImage>)>,
    /// The next frame, read ahead of its time.
    pending: Option<(f64, Arc<RenderImage>)>,
    /// The process has no more frames.
    done: bool,
}

impl VideoStream {
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

    /// The latest frame due by `clock`, if any arrived since the last call.
    /// With `first`, the first frame is taken whatever its time.
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

/// The sound as an audio source: a process per position.
struct AudioStream {
    path: PathBuf,
    duration: Option<f64>,
    process: Option<(Process, ChildStdout)>,
}

impl AudioStream {
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

/// The wall clock the pictures follow when there is no sound to follow.
#[derive(Clone, Copy)]
struct Clock {
    base: f64,
    since: Option<Instant>,
    rate: f64,
}

impl Clock {
    fn now(&self) -> f64 {
        self.base
            + self
                .since
                .map_or(0., |since| since.elapsed().as_secs_f64() * self.rate)
    }

    fn set(&mut self, position: f64, running: bool) {
        self.base = position;
        self.since = running.then(Instant::now);
    }
}

/// Whether the decoders may restart, `since` the last restart (none yet).
fn seek_due(since: Option<std::time::Duration>) -> bool {
    since.is_none_or(|since| since >= SEEK_INTERVAL)
}

/// Video through the bundled `ffmpeg`.
pub struct FfmpegPlayer {
    path: PathBuf,
    probe: Arc<Mutex<Option<Result<Info, String>>>>,
    info: Option<Info>,
    error: Option<String>,
    video: Option<VideoStream>,
    audio: Option<Engine>,
    clock: Clock,
    playing: bool,
    /// The position asked for before the file was probed.
    target: f64,
    volume: f32,
    rate: f32,
    /// A new stream's first frame is shown whatever its time.
    awaiting_first: bool,
    /// A position asked for that the decoders have not been restarted at
    /// yet, and when they last were.
    pending_seek: Option<f64>,
    last_restart: Option<Instant>,
}

impl FfmpegPlayer {
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

    /// Picks up the probe once it is done and starts the streams.
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
                // Both start where the last seek asked.
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

    /// Restarts the sound and the pictures at the position last asked for,
    /// unless they were restarted less than [`SEEK_INTERVAL`] ago; then it
    /// waits for a later call (each frame makes one), so a scrub runs at
    /// most ten restarts a second and always ends at its last position.
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

    /// The sound's clock while it runs, else the wall clock.
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
            // Stopped at the end, as a media element is.
            let position = self.position();
            self.playing = false;
            self.clock.set(position, false);
            if let Some(audio) = &self.audio {
                audio.set_paused(true);
            }
        }
        let info = self.info.clone();
        // A sound that cannot be played leaves the pictures on the wall
        // clock rather than failing the video.
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

    /// Plays a video named by `PICTO_GPUI_TEST_WEBM` through the bundled
    /// ffmpeg for a moment and checks that frames of the probed size arrive
    /// with advancing times, and that a seek moves the clock.
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
