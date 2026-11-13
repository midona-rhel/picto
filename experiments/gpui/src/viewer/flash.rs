//! Ruffle's native player owns its GPU context on a worker, never the GPUI thread.
use ruffle_core::{PlayerBuilder, events::PlayerEvent, tag_utils::SwfMovie};
use ruffle_render_wgpu::{backend::WgpuRenderBackend, target::TextureTarget};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

pub type Frame = Result<image::RgbaImage, String>;
pub fn key_descriptor(key: &str) -> ruffle_core::events::KeyDescriptor {
    use ruffle_core::events::{KeyDescriptor, KeyLocation, LogicalKey, NamedKey, PhysicalKey};
    let (physical_key, logical_key) = match key {
        "left" => (
            PhysicalKey::ArrowLeft,
            LogicalKey::Named(NamedKey::ArrowLeft),
        ),
        "right" => (
            PhysicalKey::ArrowRight,
            LogicalKey::Named(NamedKey::ArrowRight),
        ),
        "up" => (PhysicalKey::ArrowUp, LogicalKey::Named(NamedKey::ArrowUp)),
        "down" => (
            PhysicalKey::ArrowDown,
            LogicalKey::Named(NamedKey::ArrowDown),
        ),
        "enter" => (PhysicalKey::Enter, LogicalKey::Named(NamedKey::Enter)),
        "space" => (PhysicalKey::Space, LogicalKey::Character(' ')),
        "backspace" => (
            PhysicalKey::Backspace,
            LogicalKey::Named(NamedKey::Backspace),
        ),
        _ => (
            PhysicalKey::Unknown,
            key.chars()
                .next()
                .filter(|_| key.chars().count() == 1)
                .map(LogicalKey::Character)
                .unwrap_or(LogicalKey::Unknown),
        ),
    };
    KeyDescriptor {
        physical_key,
        logical_key,
        key_location: KeyLocation::Standard,
    }
}
pub struct Flash {
    pub input: mpsc::Sender<PlayerEvent>,
    control: mpsc::Sender<Control>,
    pub playback: Playback,
    pub frame: Arc<Mutex<Option<Frame>>>,
    pub traces: Arc<Mutex<std::collections::VecDeque<String>>>,
}
struct TraceLog(Arc<Mutex<std::collections::VecDeque<String>>>);
impl ruffle_core::backend::log::LogBackend for TraceLog {
    fn avm_trace(&self, message: &str) {
        let mut traces = self.0.lock().unwrap();
        if traces.len() == 64 {
            traces.pop_front();
        }
        traces.push_back(message.chars().take(512).collect());
    }
    fn avm_warning(&self, message: &str) {
        self.avm_trace(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_controls_keep_production_playback_rules() {
        let mut playback = Playback {
            playing: true,
            volume: 1.,
            muted: false,
        };
        assert_eq!(playback.toggle_play(), Control::Playing(false));
        assert_eq!(playback.toggle_play(), Control::Playing(true));
        assert_eq!(playback.stop(), Control::Stop);
        assert!(!playback.playing);
        assert_eq!(playback.set_volume(0.4), Control::Volume(0.4));
        assert_eq!(playback.toggle_mute(), Control::Volume(0.));
        assert!(playback.muted);
        // Unmuting restores the set volume; a new volume unmutes.
        assert_eq!(playback.toggle_mute(), Control::Volume(0.4));
        playback.toggle_mute();
        assert_eq!(playback.set_volume(1.5), Control::Volume(1.));
        assert!(!playback.muted);
    }

    #[test]
    #[ignore = "Requires a native GPU adapter"]
    fn stop_reloads_the_movie_at_its_first_frame() {
        let fixtures = crate::demo::fixtures().unwrap();
        let mut flash = Flash::open(fixtures.path().join("interactive.swf"));
        let deadline = Instant::now() + Duration::from_secs(20);
        let next_frame = || loop {
            if let Some(result) = flash.frame.lock().unwrap().take() {
                return result.unwrap();
            }
            assert!(Instant::now() < deadline, "Ruffle did not publish a frame");
            std::thread::sleep(Duration::from_millis(20));
        };
        let first = next_frame();
        assert_eq!(first.dimensions(), (960, 720));
        flash.toggle_play();
        flash.set_volume(0.5);
        flash.stop();
        assert!(!flash.playback.playing);
        // The reloaded movie draws its first frame again.
        std::thread::sleep(Duration::from_millis(200));
        let reloaded = loop {
            if let Some(result) = flash.frame.lock().unwrap().take() {
                break result.expect("the movie reloads");
            }
            assert!(Instant::now() < deadline, "no frame after Stop");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(reloaded.dimensions(), (960, 720));
    }
    #[test]
    #[ignore = "Requires a native GPU adapter"]
    fn real_swf_produces_frames_and_responds_to_mouse() {
        let fixtures = crate::demo::fixtures().unwrap();
        let flash = Flash::open(fixtures.path().join("interactive.swf"));
        let deadline = Instant::now() + Duration::from_secs(20);
        let wait = || {
            loop {
                if let Some(result) = flash.frame.lock().unwrap().take() {
                    return result.unwrap();
                }
                assert!(Instant::now() < deadline, "Ruffle did not publish a frame");
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let initial = loop {
            let frame = wait();
            if frame.pixels().any(|p| p != frame.get_pixel(0, 0)) {
                break frame;
            }
        };
        let output = tempfile::tempdir().unwrap();
        initial
            .save(output.path().join("ruffle-first-frame.png"))
            .unwrap();
        assert_eq!(initial.dimensions(), (960, 720));
        assert!(
            initial.pixels().any(|p| p != initial.get_pixel(0, 0)),
            "Ruffle frame is blank"
        );
        flash
            .input
            .send(PlayerEvent::MouseMove { x: 280., y: 280. })
            .unwrap();
        while !flash.traces.lock().unwrap().iter().any(|s| s == "rollOver") {
            assert!(
                Instant::now() < deadline,
                "SWF did not execute its rollOver handler"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
/// What the controls ask of the movie (`FlashPlaybackController`).
#[derive(Debug, PartialEq)]
pub enum Control {
    /// `runtime.resume()` / `runtime.suspend()`
    Playing(bool),
    /// `runtime.volume`, from 0 to 1.
    Volume(f32),
    /// Back to the first frame, suspended: production reloads the movie.
    Stop,
}

/// The controls' view of the movie, as `FlashPlayer.tsx` keeps it.
pub struct Playback {
    pub playing: bool,
    pub volume: f32,
    pub muted: bool,
}

impl Playback {
    /// `togglePlay`
    pub fn toggle_play(&mut self) -> Control {
        self.playing = !self.playing;
        Control::Playing(self.playing)
    }

    /// `stop`
    pub fn stop(&mut self) -> Control {
        self.playing = false;
        Control::Stop
    }

    /// `setVolume`: clamped to 0–1; a volume above zero unmutes.
    pub fn set_volume(&mut self, volume: f32) -> Control {
        self.volume = volume.clamp(0., 1.);
        if self.volume > 0. {
            self.muted = false;
        }
        Control::Volume(self.volume)
    }

    /// `toggleMute`: silent while muted, the set volume again after.
    pub fn toggle_mute(&mut self) -> Control {
        self.muted = !self.muted;
        Control::Volume(if self.muted { 0. } else { self.volume })
    }
}

impl Flash {
    pub fn toggle_play(&mut self) {
        let _ = self.control.send(self.playback.toggle_play());
    }

    pub fn stop(&mut self) {
        let _ = self.control.send(self.playback.stop());
    }

    pub fn set_volume(&mut self, volume: f32) {
        let _ = self.control.send(self.playback.set_volume(volume));
    }

    pub fn toggle_mute(&mut self) {
        let _ = self.control.send(self.playback.toggle_mute());
    }
}

impl Flash {
    pub fn open(path: PathBuf) -> Self {
        let (input, events) = mpsc::channel();
        let (control, controls) = mpsc::channel::<Control>();
        let frame = Arc::new(Mutex::new(None));
        let traces = Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let trace_output = traces.clone();
        let output = frame.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<()> {
                let data = std::fs::read(&path)?;
                anyhow::ensure!(
                    data.len() <= 64 * 1024 * 1024,
                    "Flash file exceeds prototype limit"
                );
                let url = url::Url::from_file_path(&path)
                    .map_err(|_| anyhow::anyhow!("Invalid Flash path"))?
                    .to_string();
                // A player for the movie, playing or held at its first frame.
                let build = |autoplay: bool, volume: f32| -> anyhow::Result<_> {
                    let movie = SwfMovie::from_data(&data, url.clone(), None, None)?;
                    let renderer = WgpuRenderBackend::<TextureTarget>::for_offscreen(
                        (960, 720),
                        wgpu::Backends::PRIMARY,
                        wgpu::PowerPreference::LowPower,
                    )
                    .map_err(|e| anyhow::anyhow!("Ruffle renderer: {e}"))?;
                    // Null navigator/storage defaults deliberately deny network and persistent writes.
                    let mut builder = PlayerBuilder::new()
                        .with_log(TraceLog(trace_output.clone()))
                        .with_movie(movie)
                        .with_renderer(renderer)
                        .with_viewport_dimensions(960, 720, 1.)
                        .with_max_execution_duration(Duration::from_millis(250))
                        .with_autoplay(autoplay);
                    match ruffle_frontend_utils::backends::audio::CpalAudioBackend::new(None) {
                        Ok(audio) => builder = builder.with_audio(audio),
                        Err(error) => {
                            eprintln!("Ruffle audio unavailable: {error}");
                        }
                    }
                    let player = builder.build();
                    player.lock().unwrap().set_volume(volume);
                    Ok(player)
                };
                let mut volume = 1.;
                let mut player = build(true, volume)?;
                let mut previous = Instant::now();
                loop {
                    let start = Instant::now();
                    let mut reload = false;
                    loop {
                        match controls.try_recv() {
                            Ok(Control::Playing(playing)) => {
                                player.lock().unwrap().set_is_playing(playing)
                            }
                            Ok(Control::Volume(level)) => {
                                volume = level;
                                player.lock().unwrap().set_volume(level);
                            }
                            Ok(Control::Stop) => reload = true,
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                        }
                    }
                    if reload {
                        player = build(false, volume)?;
                    }
                    let mut player = player.lock().unwrap();
                    loop {
                        match events.try_recv() {
                            Ok(event) => {
                                player.handle_event(event);
                            }
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                        }
                    }
                    player.tick(ruffle_common::duration::FloatDuration::from_std(
                        start - previous,
                    ));
                    previous = start;
                    if player.needs_render() {
                        player.render();
                        let renderer = (player.renderer() as &dyn std::any::Any)
                            .downcast_ref::<WgpuRenderBackend<TextureTarget>>()
                            .unwrap();
                        if let Some(image) = renderer.capture_frame() {
                            *output.lock().unwrap() = Some(Ok(image));
                        }
                    }
                    drop(player);
                    std::thread::sleep(Duration::from_millis(16).saturating_sub(start.elapsed()));
                }
            })();
            if let Err(error) = result {
                *output.lock().unwrap() = Some(Err(format!("{error:#}")));
            }
        });
        Self {
            input,
            control,
            frame,
            traces,
            playback: Playback {
                playing: true,
                volume: 1.,
                muted: false,
            },
        }
    }
}
