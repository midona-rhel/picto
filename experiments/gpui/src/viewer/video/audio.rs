//! Sound: one CPAL output stream per player, the output the Flash player
//! already uses, fed by a worker thread that decodes, fits the samples to the
//! device and stretches them for the playback rate. The samples played are
//! the clock: the position is the seek target plus what the device has
//! consumed since, scaled by the rate. The last samples played are kept for
//! the visualizer, as the media element's analyser sees them.
use super::backend::{Backend, Frame, Status};
use super::dsp::{Resampler, Stretch, map_channels};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

/// How much decoded sound the worker keeps ahead of the device.
const AHEAD: f64 = 0.25;
/// How many recent samples the visualizer can read: the oscilloscope's
/// 1024-sample window and some room.
const RECENT: usize = 4096;

/// A decoder of interleaved `f32` samples.
pub trait Source: Send {
    /// Sample rate and channel count.
    fn spec(&self) -> (u32, usize);
    fn duration(&self) -> Option<f64>;
    /// Moves to `seconds`; the next chunk starts there.
    fn seek(&mut self, seconds: f64) -> Result<(), String>;
    /// The next chunk, `None` at the end.
    fn next_chunk(&mut self) -> Option<Result<Vec<f32>, String>>;
}

/// Opens a source starting at a position.
pub type OpenSource = Box<dyn Fn(f64) -> Result<Box<dyn Source>, String> + Send>;

/// The samples most recently played, mixed to mono, for the visualizer.
#[derive(Clone)]
pub struct Analysis(Arc<Mutex<Recent>>);

struct Recent {
    samples: Vec<f32>,
    at: usize,
    /// Bumped whenever samples arrive, so a reader can tell new data from old.
    written: u64,
}

impl Analysis {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Recent {
            samples: vec![0.; RECENT],
            at: 0,
            written: 0,
        })))
    }

    /// The last `count` samples, oldest first.
    pub fn latest(&self, count: usize) -> Vec<f32> {
        let recent = self.0.lock().unwrap();
        let count = count.min(RECENT);
        (0..count)
            .map(|i| recent.samples[(recent.at + RECENT - count + i) % RECENT])
            .collect()
    }

    /// A count that changes whenever new samples have been played.
    pub fn written(&self) -> u64 {
        self.0.lock().unwrap().written
    }

    fn push(&self, interleaved: &[f32], channels: usize) {
        let mut recent = self.0.lock().unwrap();
        for frame in interleaved.chunks(channels) {
            let mono = frame.iter().sum::<f32>() / channels as f32;
            let at = recent.at;
            recent.samples[at] = mono;
            recent.at = (at + 1) % RECENT;
        }
        recent.written += 1;
    }
}

/// What the device plays from, and the clock it keeps.
struct Buffer {
    samples: VecDeque<f32>,
    /// Frames the device has taken since `base`.
    played: u64,
    /// The media position the queue started at.
    base: f64,
    rate: f64,
    device_rate: u32,
    channels: usize,
    /// The source has run out; what is queued is all there is.
    input_ended: bool,
    duration: Option<f64>,
    error: Option<String>,
    /// The output device is open.
    open: bool,
}

impl Buffer {
    fn position(&self) -> f64 {
        let played = self.played as f64 / self.device_rate.max(1) as f64 * self.rate;
        let position = self.base + played;
        self.duration
            .map_or(position, |duration| position.min(duration))
    }

    fn ended(&self) -> bool {
        self.input_ended && self.samples.is_empty()
    }
}

struct Shared {
    buffer: Mutex<Buffer>,
    paused: AtomicBool,
    /// The gain applied as samples go out, as `f32` bits.
    volume: AtomicU32,
    analysis: Analysis,
}

enum Command {
    Seek(f64),
    Rate(f32),
}

/// What a run of queued commands comes to: the last position asked for
/// and the last rate. A scrub queues a seek per pointer move; restarting the
/// source once for the run, not once per move, keeps the worker (and, for
/// video through ffmpeg, the processes it starts) to one restart at a time.
fn folded(commands: impl IntoIterator<Item = Command>) -> (Option<f64>, Option<f32>) {
    let (mut seek, mut rate) = (None, None);
    for command in commands {
        match command {
            Command::Seek(target) => seek = Some(target),
            Command::Rate(new) => rate = Some(new),
        }
    }
    (seek, rate)
}

/// A source played through the default output device.
pub struct Engine {
    shared: Arc<Shared>,
    commands: mpsc::Sender<Command>,
}

impl Engine {
    /// Starts the worker, which opens the device and the source; this
    /// returns at once.
    pub fn start(open: OpenSource, start: f64, rate: f32, paused: bool) -> Self {
        let shared = Arc::new(Shared {
            buffer: Mutex::new(Buffer {
                samples: VecDeque::new(),
                played: 0,
                base: start,
                rate: rate as f64,
                device_rate: 48_000,
                channels: 2,
                input_ended: false,
                duration: None,
                error: None,
                open: false,
            }),
            paused: AtomicBool::new(paused),
            volume: AtomicU32::new(0f32.to_bits()),
            analysis: Analysis::new(),
        });
        let (commands, receiver) = mpsc::channel();
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("picto-audio".into())
            .spawn(move || {
                if let Err(error) = run(worker.clone(), open, start, rate, receiver) {
                    let mut buffer = worker.buffer.lock().unwrap();
                    buffer.error = Some(error);
                    buffer.input_ended = true;
                    buffer.samples.clear();
                }
            })
            .expect("audio worker thread");
        Self { shared, commands }
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared
            .volume
            .store(volume.clamp(0., 1.).to_bits(), Ordering::Relaxed);
    }

    pub fn seek(&self, seconds: f64) {
        // The clock moves at once; the worker refills from there.
        {
            let mut buffer = self.shared.buffer.lock().unwrap();
            buffer.samples.clear();
            buffer.played = 0;
            buffer.base = seconds;
            buffer.input_ended = false;
        }
        let _ = self.commands.send(Command::Seek(seconds));
    }

    pub fn set_rate(&self, rate: f32) {
        let _ = self.commands.send(Command::Rate(rate));
    }

    pub fn position(&self) -> f64 {
        self.shared.buffer.lock().unwrap().position()
    }

    pub fn duration(&self) -> Option<f64> {
        self.shared.buffer.lock().unwrap().duration
    }

    pub fn ended(&self) -> bool {
        self.shared.buffer.lock().unwrap().ended()
    }

    /// The device is open and the clock runs on its samples.
    pub fn running(&self) -> bool {
        let buffer = self.shared.buffer.lock().unwrap();
        buffer.open && buffer.error.is_none()
    }

    pub fn error(&self) -> Option<String> {
        self.shared.buffer.lock().unwrap().error.clone()
    }

    pub fn analysis(&self) -> Analysis {
        self.shared.analysis.clone()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Dropping the sender ends the worker, which drops the stream.
        self.shared.paused.store(true, Ordering::Relaxed);
    }
}

/// Opens the default output device, playing from the shared buffer.
fn open_output(shared: &Arc<Shared>) -> Result<(cpal::Stream, u32, usize), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No audio output device")?;
    let config = device
        .default_output_config()
        .map_err(|error| format!("Audio output: {error}"))?;
    let format = config.sample_format();
    let config = cpal::StreamConfig::from(config);
    let (rate, channels) = (config.sample_rate.0, config.channels as usize);
    fn build<T: cpal::SizedSample + cpal::FromSample<f32>>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        shared: Arc<Shared>,
    ) -> Result<cpal::Stream, cpal::BuildStreamError> {
        let channels = config.channels as usize;
        let mut scratch = Vec::new();
        device.build_output_stream(
            config,
            move |out: &mut [T], _| {
                scratch.clear();
                if !shared.paused.load(Ordering::Relaxed) {
                    let mut buffer = shared.buffer.lock().unwrap();
                    let take = out.len().min(buffer.samples.len()) / channels * channels;
                    scratch.extend(buffer.samples.drain(..take));
                    buffer.played += (take / channels) as u64;
                }
                if !scratch.is_empty() {
                    shared.analysis.push(&scratch, channels);
                }
                let gain = f32::from_bits(shared.volume.load(Ordering::Relaxed));
                for (i, sample) in out.iter_mut().enumerate() {
                    let value = scratch.get(i).copied().unwrap_or(0.) * gain;
                    *sample = T::from_sample(value);
                }
            },
            |error| eprintln!("Audio output error: {error}"),
            None,
        )
    }
    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, shared.clone()),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, shared.clone()),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, shared.clone()),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, shared.clone()),
        other => return Err(format!("Unsupported audio output format {other:?}")),
    }
    .map_err(|error| format!("Audio output: {error}"))?;
    stream
        .play()
        .map_err(|error| format!("Audio output: {error}"))?;
    Ok((stream, rate, channels))
}

/// The worker: owns the stream and the source, and keeps the queue a little
/// ahead of the device.
fn run(
    shared: Arc<Shared>,
    open: OpenSource,
    start: f64,
    mut rate: f32,
    commands: mpsc::Receiver<Command>,
) -> Result<(), String> {
    let mut source = open(start)?;
    let (_stream, device_rate, device_channels) = open_output(&shared)?;
    {
        let mut buffer = shared.buffer.lock().unwrap();
        buffer.device_rate = device_rate;
        buffer.channels = device_channels;
        buffer.duration = source.duration();
        buffer.open = true;
    }
    let (source_rate, source_channels) = source.spec();
    let pipeline = |rate: f32| {
        (
            Resampler::new(device_channels, source_rate, device_rate),
            Stretch::new(device_channels, device_rate, rate),
        )
    };
    let (mut resampler, mut stretch) = pipeline(rate);
    let ahead = (AHEAD * device_rate as f64) as usize * device_channels;
    let mut finished = false;
    loop {
        // While the queue is full (or the source done), wait for a command;
        // otherwise only look for one.
        let queued = shared.buffer.lock().unwrap().samples.len();
        let command = if finished || queued >= ahead {
            match commands.recv_timeout(Duration::from_millis(10)) {
                Ok(command) => Some(command),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        } else {
            match commands.try_recv() {
                Ok(command) => Some(command),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        };
        // A seek or a new rate restarts the pipeline from that position.
        if let Some(command) = command {
            let (seek, new_rate) = folded(
                std::iter::once(command).chain(std::iter::from_fn(|| commands.try_recv().ok())),
            );
            if let Some(new_rate) = new_rate {
                rate = new_rate;
            }
            let target = seek.unwrap_or_else(|| shared.buffer.lock().unwrap().position());
            source.seek(target)?;
            (resampler, stretch) = pipeline(rate);
            finished = false;
            let mut buffer = shared.buffer.lock().unwrap();
            buffer.samples.clear();
            buffer.played = 0;
            buffer.base = target;
            buffer.rate = rate as f64;
            buffer.input_ended = false;
            continue;
        }
        let samples = match source.next_chunk() {
            Some(Ok(chunk)) => {
                let mapped = map_channels(&chunk, source_channels, device_channels);
                stretch.process(&resampler.process(&mapped))
            }
            Some(Err(error)) => return Err(error),
            None => {
                finished = true;
                stretch.finish()
            }
        };
        let mut buffer = shared.buffer.lock().unwrap();
        buffer.samples.extend(samples);
        if finished {
            buffer.input_ended = true;
        }
    }
}

/// Audio files decoded in-process by Symphonia.
pub struct SymphoniaSource {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track: u32,
    rate: u32,
    channels: usize,
    duration: Option<f64>,
    /// Samples before this timestamp are dropped after a seek.
    skip_until: u64,
}

impl SymphoniaSource {
    pub fn open(path: &Path) -> Result<Self, String> {
        use symphonia::core::{
            codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream,
            meta::MetadataOptions, probe::Hint,
        };
        let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
        let stream = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(extension);
        }
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions {
                    enable_gapless: true,
                    ..Default::default()
                },
                &MetadataOptions::default(),
            )
            .map_err(|error| format!("Unsupported audio: {error}"))?;
        let format = probed.format;
        let track = format
            .default_track()
            .ok_or("The file has no audio track")?;
        let params = &track.codec_params;
        let rate = params.sample_rate.ok_or("Unknown sample rate")?;
        let channels = params.channels.map_or(2, |channels| channels.count());
        let time_base = params.time_base;
        let duration = match (params.n_frames, time_base) {
            (Some(frames), Some(base)) => {
                let time = base.calc_time(frames);
                Some(time.seconds as f64 + time.frac)
            }
            (Some(frames), None) => Some(frames as f64 / rate as f64),
            _ => None,
        };
        let decoder = symphonia::default::get_codecs()
            .make(params, &DecoderOptions::default())
            .map_err(|error| format!("Unsupported audio codec: {error}"))?;
        let track = track.id;
        Ok(Self {
            format,
            decoder,
            track,
            rate,
            channels,
            duration,
            skip_until: 0,
        })
    }
}

impl Source for SymphoniaSource {
    fn spec(&self) -> (u32, usize) {
        (self.rate, self.channels)
    }

    fn duration(&self) -> Option<f64> {
        self.duration
    }

    fn seek(&mut self, seconds: f64) -> Result<(), String> {
        use symphonia::core::formats::{SeekMode, SeekTo};
        use symphonia::core::units::Time;
        let time = Time::new(seconds.max(0.).floor() as u64, seconds.max(0.).fract());
        match self.format.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time,
                track_id: Some(self.track),
            },
        ) {
            Ok(seeked) => {
                self.skip_until = seeked.required_ts;
                self.decoder.reset();
                Ok(())
            }
            // A seek past the end leaves nothing to play.
            Err(symphonia::core::errors::Error::SeekError(_)) => {
                self.skip_until = u64::MAX;
                Ok(())
            }
            Err(error) => Err(format!("Seek failed: {error}")),
        }
    }

    fn next_chunk(&mut self) -> Option<Result<Vec<f32>, String>> {
        use symphonia::core::audio::SampleBuffer;
        use symphonia::core::errors::Error;
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return None;
                }
                Err(Error::ResetRequired) => return None,
                Err(error) => return Some(Err(error.to_string())),
            };
            if packet.track_id() != self.track {
                continue;
            }
            let (start, length) = (packet.ts(), packet.dur());
            if start + length <= self.skip_until && self.skip_until != 0 {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                // A damaged packet is skipped, as players do.
                Err(Error::DecodeError(_)) => continue,
                Err(error) => return Some(Err(error.to_string())),
            };
            let spec = *decoded.spec();
            let channels = spec.channels.count();
            let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            samples.copy_interleaved_ref(decoded);
            let mut samples = samples.samples().to_vec();
            if self.skip_until > start && self.skip_until != u64::MAX {
                let skip = ((self.skip_until - start) as usize * channels).min(samples.len());
                samples.drain(..skip);
            }
            self.skip_until = 0;
            if channels != self.channels {
                samples = map_channels(&samples, channels, self.channels);
            }
            return Some(Ok(samples));
        }
    }
}

/// An audio file: Symphonia's decoder through the engine.
pub struct AudioPlayer {
    engine: Engine,
    playing: bool,
    path: PathBuf,
}

impl AudioPlayer {
    pub fn open(path: &Path) -> Result<Self, String> {
        let path = path.to_path_buf();
        let source_path = path.clone();
        let open: OpenSource = Box::new(move |start| {
            let mut source = SymphoniaSource::open(&source_path)?;
            if start > 0. {
                source.seek(start)?;
            }
            Ok(Box::new(source) as Box<dyn Source>)
        });
        Ok(Self {
            engine: Engine::start(open, 0., 1., true),
            playing: false,
            path,
        })
    }
}

impl Backend for AudioPlayer {
    fn play(&mut self) {
        self.playing = true;
        self.engine.set_paused(false);
    }

    fn pause(&mut self) {
        self.playing = false;
        self.engine.set_paused(true);
    }

    fn seek(&mut self, seconds: f64) {
        self.engine.seek(seconds);
    }

    fn set_rate(&mut self, rate: f32) {
        self.engine.set_rate(rate);
    }

    fn set_volume(&mut self, volume: f32, muted: bool) {
        self.engine.set_volume(if muted { 0. } else { volume });
    }

    fn status(&mut self) -> Status {
        let ended = self.engine.ended();
        let duration = self.engine.duration().unwrap_or(0.);
        let error = self.engine.error().map(|error| {
            format!(
                "{error} ({})",
                self.path.file_name().unwrap_or_default().to_string_lossy()
            )
        });
        Status {
            position: self.engine.position(),
            duration,
            // The whole file is local.
            buffered: duration,
            playing: self.playing && !ended && error.is_none(),
            ended,
            error,
            size: None,
            fps: None,
        }
    }

    fn take_frame(&mut self) -> Option<Frame> {
        None
    }

    fn analysis(&self) -> Option<Analysis> {
        Some(self.engine.analysis())
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioPlayer, Backend, Command, Source, SymphoniaSource, folded};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    #[test]
    fn queued_seeks_and_rates_come_to_the_last_of_each() {
        let commands = [
            Command::Seek(1.),
            Command::Rate(1.5),
            Command::Seek(2.),
            Command::Seek(3.5),
        ];
        assert_eq!(folded(commands), (Some(3.5), Some(1.5)));
        assert_eq!(folded([Command::Rate(2.)]), (None, Some(2.)));
    }

    /// Plays the file in `PICTO_GPUI_TEST_AUDIO` silently through the
    /// default output device: the clock follows the samples played, a seek
    /// moves it, and at double speed it runs twice as fast.
    #[test]
    #[ignore = "needs an audio file in PICTO_GPUI_TEST_AUDIO and an output device"]
    fn plays_through_the_output_device() {
        let path = PathBuf::from(std::env::var("PICTO_GPUI_TEST_AUDIO").unwrap());
        let mut player = AudioPlayer::open(&path).unwrap();
        player.set_volume(0., true);
        player.play();
        let measure = |player: &mut AudioPlayer, wait: Duration| {
            let start = player.status().position;
            let since = Instant::now();
            std::thread::sleep(wait);
            (player.status().position - start) / since.elapsed().as_secs_f64()
        };
        // Let the device start.
        std::thread::sleep(Duration::from_millis(300));
        let status = player.status();
        assert!(status.error.is_none(), "{status:?}");
        let speed = measure(&mut player, Duration::from_millis(1000));
        assert!((speed - 1.).abs() < 0.15, "normal speed ran at {speed}");
        player.seek(1.);
        assert!((player.status().position - 1.).abs() < 0.01);
        player.set_rate(2.);
        std::thread::sleep(Duration::from_millis(300));
        let speed = measure(&mut player, Duration::from_millis(1000));
        assert!((speed - 2.).abs() < 0.3, "double speed ran at {speed}");
        player.pause();
        std::thread::sleep(Duration::from_millis(100));
        let speed = measure(&mut player, Duration::from_millis(300));
        assert!(speed.abs() < 0.01, "paused but moving at {speed}");
        eprintln!("{}: {:?}", path.display(), player.status());
    }

    /// Decodes an audio file named by `PICTO_GPUI_TEST_AUDIO`, seeks into it
    /// and checks the samples keep coming from there.
    #[test]
    #[ignore = "needs an audio file in PICTO_GPUI_TEST_AUDIO"]
    fn decodes_and_seeks_a_real_audio_file() {
        let path = PathBuf::from(std::env::var("PICTO_GPUI_TEST_AUDIO").unwrap());
        let mut source = SymphoniaSource::open(&path).unwrap();
        let (rate, channels) = source.spec();
        assert!(rate > 0 && channels > 0);
        let mut frames = 0;
        while let Some(chunk) = source.next_chunk() {
            frames += chunk.unwrap().len() / channels;
            if frames > rate as usize {
                break;
            }
        }
        assert!(frames > rate as usize, "a second of sound decodes");
        source.seek(1.).unwrap();
        let chunk = source.next_chunk().unwrap().unwrap();
        assert!(!chunk.is_empty());
        eprintln!(
            "{}: {rate} Hz, {channels} channels, {:?} s",
            path.display(),
            source.duration()
        );
    }
}
