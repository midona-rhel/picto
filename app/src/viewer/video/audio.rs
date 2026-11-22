//! The sound of the media player. Each player has one output stream of
//! CPAL, which the Flash player also uses for its sound. A worker thread
//! decodes the audio, fits the samples to the output device's channels and
//! sample rate, and stretches them for the playback rate. The playback
//! position is computed from the samples that the device has played: it is
//! the position of the last seek, plus the time the device has played since
//! then multiplied by the playback rate. The samples that were played last
//! are kept for the audio visualizer.
use super::backend::{Backend, Frame, Status};
use super::dsp::{Resampler, Stretch, map_channels};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

/// How much sound, in seconds, the worker keeps decoded and queued ahead of
/// what the device is playing.
const AHEAD: f64 = 0.25;
/// How many of the samples played last are kept for the visualizer. The
/// oscilloscope reads the most at a time, 1024 samples.
const RECENT: usize = 4096;

/// A decoder that produces the audio of one file as interleaved `f32`
/// samples.
pub trait Source: Send {
    /// Returns the sample rate and the number of channels.
    fn spec(&self) -> (u32, usize);
    /// Returns the length of the audio in seconds, or `None` when it is not
    /// known.
    fn duration(&self) -> Option<f64>;
    /// Moves to the position `seconds`. The next chunk starts there.
    fn seek(&mut self, seconds: f64) -> Result<(), String>;
    /// Returns the next chunk of samples, or `None` at the end of the
    /// audio.
    fn next_chunk(&mut self) -> Option<Result<Vec<f32>, String>>;
}

/// A function that opens a source at the position it is given, in seconds.
pub type OpenSource = Box<dyn Fn(f64) -> Result<Box<dyn Source>, String> + Send>;

/// The samples that the output device played last, mixed to mono. The audio
/// visualizer reads them.
#[derive(Clone)]
pub struct Analysis(Arc<Mutex<Recent>>);

/// The last `RECENT` mono samples, kept in a ring.
struct Recent {
    /// The samples. The oldest one is at the index `at`.
    samples: Vec<f32>,
    /// The index at which the next sample is written.
    at: usize,
    /// Raised by 1 each time samples arrive, so that a reader can tell
    /// whether new samples have arrived since it last read.
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

    /// Returns the `count` samples that were played last, the oldest
    /// first. `count` is limited to `RECENT`.
    pub fn latest(&self, count: usize) -> Vec<f32> {
        let recent = self.0.lock().unwrap();
        let count = count.min(RECENT);
        (0..count)
            .map(|i| recent.samples[(recent.at + RECENT - count + i) % RECENT])
            .collect()
    }

    /// Returns a count that changes each time new samples have been played.
    pub fn written(&self) -> u64 {
        self.0.lock().unwrap().written
    }

    /// Adds the interleaved samples of `channels` channels that the device
    /// has just taken. Each frame is stored as the mean of its channels.
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

/// The queue of samples that the output device plays from, and the values
/// that the playback position is computed from.
struct Buffer {
    /// The interleaved samples that are queued for the device.
    samples: VecDeque<f32>,
    /// The number of frames the device has taken since the position `base`.
    played: u64,
    /// The position in the media, in seconds, of the first sample that was
    /// queued after the queue was last emptied.
    base: f64,
    /// The playback rate that the queued samples were stretched for.
    rate: f64,
    /// The sample rate of the output device.
    device_rate: u32,
    channels: usize,
    /// True when the source has reached its end, so that no more samples
    /// will be queued.
    input_ended: bool,
    duration: Option<f64>,
    /// The error that stopped the worker, or `None`.
    error: Option<String>,
    /// True once the output device is open.
    open: bool,
}

impl Buffer {
    /// Returns the playback position in seconds: `base` plus the time the
    /// device has played since then multiplied by the rate. It is not more
    /// than the length of the audio.
    fn position(&self) -> f64 {
        let played = self.played as f64 / self.device_rate.max(1) as f64 * self.rate;
        let position = self.base + played;
        self.duration
            .map_or(position, |duration| position.min(duration))
    }

    /// Returns true when the source has reached its end and the device has
    /// played everything that was queued.
    fn ended(&self) -> bool {
        self.input_ended && self.samples.is_empty()
    }
}

/// What the engine, the worker thread and the output stream share.
struct Shared {
    buffer: Mutex<Buffer>,
    /// True while playback is paused. The output stream then takes no
    /// samples from the queue and plays silence.
    paused: AtomicBool,
    /// The volume, from 0 to 1, that each sample is multiplied by as it
    /// goes to the device. It is stored as the bits of an `f32`.
    volume: AtomicU32,
    analysis: Analysis,
}

/// What the engine asks of the worker: a seek to a position in seconds, or
/// a new playback rate.
enum Command {
    Seek(f64),
    Rate(f32),
}

/// Returns what a series of queued commands amounts to: the position of the
/// last seek and the last rate, each `None` when the series has none. A
/// drag of the progress bar queues one seek for each move of the pointer.
/// The worker restarts its source once for all the commands that are
/// queued, and not once for each of them. For a video that `ffmpeg` plays,
/// each restart starts a new `ffmpeg` process, so this also keeps down the
/// number of processes that are started.
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

/// Plays one source through the default output device.
pub struct Engine {
    shared: Arc<Shared>,
    commands: mpsc::Sender<Command>,
}

impl Engine {
    /// Starts the worker thread, which opens the source at `start` seconds
    /// and the output device, and returns at once. `rate` is the playback
    /// rate, and `paused` says whether playback is paused at first. The
    /// volume is 0 until `set_volume` is called. When the worker fails, its
    /// error is recorded and `error` returns it.
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

    /// Pauses playback or continues it.
    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
    }

    /// Sets the volume, limited to the range from 0 to 1.
    pub fn set_volume(&self, volume: f32) {
        self.shared
            .volume
            .store(volume.clamp(0., 1.).to_bits(), Ordering::Relaxed);
    }

    /// Moves the position to `seconds`.
    pub fn seek(&self, seconds: f64) {
        // The queue is emptied and the position is set here, so that
        // `position` returns the new position at once. The worker then
        // restarts the source at that position and fills the queue again.
        {
            let mut buffer = self.shared.buffer.lock().unwrap();
            buffer.samples.clear();
            buffer.played = 0;
            buffer.base = seconds;
            buffer.input_ended = false;
        }
        let _ = self.commands.send(Command::Seek(seconds));
    }

    /// Sets the playback rate.
    pub fn set_rate(&self, rate: f32) {
        let _ = self.commands.send(Command::Rate(rate));
    }

    /// Returns the playback position in seconds.
    pub fn position(&self) -> f64 {
        self.shared.buffer.lock().unwrap().position()
    }

    /// Returns the length of the audio in seconds, or `None` while it is
    /// not known.
    pub fn duration(&self) -> Option<f64> {
        self.shared.buffer.lock().unwrap().duration
    }

    /// Returns true when the source has reached its end and everything that
    /// was queued has been played.
    pub fn ended(&self) -> bool {
        self.shared.buffer.lock().unwrap().ended()
    }

    /// Returns true when the output device is open and no error has
    /// occurred. The position then advances as the device plays samples.
    pub fn running(&self) -> bool {
        let buffer = self.shared.buffer.lock().unwrap();
        buffer.open && buffer.error.is_none()
    }

    /// Returns the error that stopped the worker, or `None`.
    pub fn error(&self) -> Option<String> {
        self.shared.buffer.lock().unwrap().error.clone()
    }

    /// Returns access to the samples that were played last, for the
    /// visualizer.
    pub fn analysis(&self) -> Analysis {
        self.shared.analysis.clone()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // The output stream stops taking samples at once. After this
        // function the engine's sender of commands is dropped. The worker
        // then finds its channel disconnected and returns, which drops the
        // output stream.
        self.shared.paused.store(true, Ordering::Relaxed);
    }
}

/// Opens the default output device with its default configuration and
/// starts a stream that plays the samples of the shared queue. Returns the
/// stream, the device's sample rate and its number of channels. Each time
/// the device asks for samples, the stream takes whole frames from the
/// queue, unless playback is paused, counts them, adds them to the samples
/// for the visualizer, and multiplies them by the volume. What the device
/// asked for beyond that is filled with silence.
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

/// The body of the worker thread. It opens the source and the output
/// device. Then it keeps 0.25 s of sound (`AHEAD`) queued ahead of the
/// device: it takes chunks from the source, fits them to the device's
/// channels and sample rate, stretches them for the playback rate and adds
/// them to the queue. It returns when the engine has been dropped, or with
/// an error when the source or the device fails.
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
        // While enough sound is queued, or the source has ended, the worker
        // waits up to 10 ms for a command. Otherwise it takes a command
        // only if one is waiting, and goes on to decode the next chunk.
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
        // For a seek or a new rate, all the commands that are waiting are
        // taken together. The source seeks to the position of the last
        // seek, or to the current position when only the rate changed. The
        // resampler and the stretcher are made again, and the queue is
        // emptied and starts at that position.
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

/// A source that decodes an audio file inside the app's process with the
/// Symphonia library.
pub struct SymphoniaSource {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track: u32,
    rate: u32,
    channels: usize,
    duration: Option<f64>,
    /// After a seek, the timestamp of the first sample to return. The
    /// samples before it are dropped. It is 0 when no samples are to be
    /// dropped, and `u64::MAX` after a seek past the end, when all the
    /// samples that follow are dropped.
    skip_until: u64,
}

impl SymphoniaSource {
    /// Opens the audio file `path` and makes a decoder for its default
    /// track.
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
            // A seek to a position past the end fails with a seek error.
            // All the samples that follow are then dropped, so the audio
            // ends.
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
                // A packet that cannot be decoded is skipped, and decoding
                // goes on with the next packet.
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

/// The backend for an audio file. It plays a `SymphoniaSource` through an
/// `Engine`.
pub struct AudioPlayer {
    engine: Engine,
    playing: bool,
    path: PathBuf,
}

impl AudioPlayer {
    /// Makes a player for the audio file `path`. Playback is paused at
    /// first. The worker thread opens the file, so an error in opening it
    /// is reported later, through `status`.
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
            // The file is on disk, so all of it counts as buffered.
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

    /// Plays the file named by the environment variable
    /// `PICTO_GPUI_TEST_AUDIO` through the default output device, with the
    /// volume at 0. It checks that the position advances by about 1 s a
    /// second at the rate 1, that a seek changes the position at once, that
    /// the position advances by about 2 s a second at the rate 2, and that
    /// it does not advance while playback is paused.
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
        // The worker needs some time to open the output device.
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

    /// Decodes more than one second of the audio file named by the
    /// environment variable `PICTO_GPUI_TEST_AUDIO`. Then it seeks to 1 s
    /// and checks that the next chunk holds samples.
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
