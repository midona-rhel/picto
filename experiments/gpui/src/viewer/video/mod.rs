//! The media player (`VideoPlayer.tsx`) for video and audio items: the
//! picture (or, for audio, the visualizer) filling the pane, and the
//! controls along the bottom that hide 2.5 s after the pointer stops: the
//! scrubber with its buffered and played ranges and the time under the
//! pointer (or the waveform, for audio), play and the 5 s skips, the time,
//! the speed menu, loop, the volume slider and mute, and fullscreen; the
//! volume display that keys bring up; and the player's keys.
//!
//! The player knows media only through [`backend::Backend`]; decoding never
//! runs on the thread that draws.
pub mod audio;
#[cfg(target_os = "macos")]
mod avf;
pub mod backend;
pub(super) mod button;
pub(super) mod controls;
mod dsp;
mod ffmpeg;
pub mod model;
mod visualizer;

use crate::i18n::t;
use crate::theme::Text as _;
use crate::{motion, theme, ui};
use backend::{Backend, Frame, Status};
use button::{label_button, media_button};
use gpui::*;
use model::{
    ControlsVisibility, Kind, Options, PLAYBACK_RATES, Playback, SKIP_STEP, VOLUME_HUD_DURATION,
    VOLUME_STEP,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

actions!(
    video,
    [
        TogglePlay,
        SeekBackward,
        SeekForward,
        StepFrameBackward,
        StepFrameForward,
        VolumeUp,
        VolumeDown,
        ToggleMute,
        ToggleLoop,
        RateIncrease,
        RateDecrease,
        RateReset,
        ToggleFullscreen,
        ExitFullscreen
    ]
);

/// The key context of the player while it has its window to itself.
const FULLSCREEN_CONTEXT: &str = "PlayerFullscreen";

/// Escape leaves the player's fullscreen, as the browser's own key does,
/// before anything else in the window sees it.
pub fn bind(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        ExitFullscreen,
        Some(FULLSCREEN_CONTEXT),
    )]);
}

/// `requestFullscreen` on the player's element: the window goes full screen
/// and shows the player alone, its controls and keys still its own.
struct Fullscreen {
    window: AnyWindowHandle,
    player: WeakEntity<VideoPlayer>,
    focus: FocusHandle,
    /// What had the keyboard before, to give it back.
    return_focus: Option<FocusHandle>,
    /// The window was already full screen (View > Enter Full Screen), and
    /// stays so afterwards.
    was_full_screen: bool,
    /// The window has been seen full screen since, so its leaving full
    /// screen by other means (the green button) ends this too.
    seen_full_screen: bool,
    /// The player left its host (another item opened): this ends at the
    /// next frame, as removing the fullscreen element does.
    released: bool,
}

#[derive(Default)]
struct FullscreenState(Option<Fullscreen>);

impl Global for FullscreenState {}

/// Whether `player` is the one shown over its whole window.
pub fn is_fullscreen(player: &Entity<VideoPlayer>, cx: &App) -> bool {
    cx.try_global::<FullscreenState>()
        .and_then(|state| state.0.as_ref())
        .is_some_and(|fullscreen| !fullscreen.released && fullscreen.player == player.downgrade())
}

/// The player's fullscreen button, F and a double-click: shows a video's
/// player alone over its window, or goes back. Audio has no fullscreen.
pub fn toggle_fullscreen(player: &Entity<VideoPlayer>, window: &mut Window, cx: &mut App) {
    if is_fullscreen(player, cx) {
        exit_fullscreen(window, cx);
    } else {
        enter_fullscreen(player.downgrade(), window, cx);
    }
}

fn enter_fullscreen(player: WeakEntity<VideoPlayer>, window: &mut Window, cx: &mut App) {
    // Another player still holding a window gives it back first.
    if cx
        .try_global::<FullscreenState>()
        .is_some_and(|state| state.0.is_some())
    {
        exit_fullscreen(window, cx);
    }
    let was_full_screen = window.is_fullscreen();
    let focus = cx.focus_handle();
    let return_focus = window.focused(cx);
    cx.set_global(FullscreenState(Some(Fullscreen {
        window: window.window_handle(),
        player,
        focus: focus.clone(),
        return_focus,
        was_full_screen,
        seen_full_screen: was_full_screen,
        released: false,
    })));
    if !was_full_screen {
        window.toggle_fullscreen();
    }
    window.focus(&focus, cx);
    window.refresh();
}

/// Leaves the player's fullscreen: the window leaves full screen unless it
/// was full screen before, and the keyboard goes back where it was.
pub fn exit_fullscreen(window: &mut Window, cx: &mut App) {
    let Some(fullscreen) = cx.default_global::<FullscreenState>().0.take() else {
        return;
    };
    let restore = move |window: &mut Window, cx: &mut App| {
        if !fullscreen.was_full_screen && window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        if let Some(focus) = &fullscreen.return_focus {
            window.focus(focus, cx);
        }
        window.refresh();
    };
    // The window being drawn or handling input cannot be updated from
    // inside itself.
    if window.window_handle() == fullscreen.window {
        restore(window, cx);
    } else {
        let handle = fullscreen.window;
        let _ = handle.update(cx, |_, window, cx| restore(window, cx));
    }
}

/// Ends `player`'s fullscreen at the next frame: its host let it go.
pub fn release_fullscreen(player: &Entity<VideoPlayer>, cx: &mut App) {
    if let Some(fullscreen) = cx.default_global::<FullscreenState>().0.as_mut()
        && fullscreen.player == player.downgrade()
    {
        fullscreen.released = true;
    }
}

/// The player over the whole of `window` while it is fullscreen there: on
/// black, as the fullscreen element shows. Hosts put it last in their root.
pub fn fullscreen_layer(window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let handle = window.window_handle();
    let full_screen = window.is_fullscreen();
    let state = cx.default_global::<FullscreenState>();
    let fullscreen = state
        .0
        .as_mut()
        .filter(|fullscreen| fullscreen.window == handle)?;
    fullscreen.seen_full_screen |= full_screen;
    let player = fullscreen.player.upgrade();
    let ended =
        fullscreen.released || player.is_none() || (fullscreen.seen_full_screen && !full_screen);
    if ended {
        window.defer(cx, exit_fullscreen);
        return None;
    }
    let player = player?;
    let focus = fullscreen.focus.clone();
    let weak = player.downgrade();
    let player_for_exit = weak.clone();
    let layer = div()
        .id("media-fullscreen-layer")
        .absolute()
        .inset_0()
        .occlude()
        .bg(rgba(theme::MEDIA_BACKGROUND))
        .track_focus(&focus)
        .key_context(key_context(FULLSCREEN_CONTEXT, true))
        .on_action(move |_: &ExitFullscreen, window, cx| {
            if let Some(player) = player_for_exit.upgrade() {
                player.update(cx, |player, cx| player.reveal(cx));
            }
            exit_fullscreen(window, cx)
        })
        .child(player);
    Some(on_actions(layer, move |_| weak.upgrade()).into_any_element())
}

/// Whether a media type plays in this player, and as what.
pub fn kind_of(mime: &str) -> Option<Kind> {
    if mime.starts_with("video/") {
        Some(Kind::Video)
    } else if mime.starts_with("audio/") {
        Some(Kind::Audio)
    } else {
        None
    }
}

/// A viewer's key context, with `Media` added while it shows the player,
/// for the player's keys (`keymap::Scope::Media`).
pub fn key_context(base: &'static str, showing: bool) -> KeyContext {
    let mut context = KeyContext::default();
    context.add(base);
    if showing {
        context.add("Media"); // i18n: a key context
    }
    context
}

/// Forwards the player's actions from the element that has the keyboard
/// (a viewer) to the player, if one is showing.
pub fn on_actions<E: InteractiveElement>(
    element: E,
    player: impl Fn(&App) -> Option<Entity<VideoPlayer>> + Clone + 'static,
) -> E {
    fn forward<A: Action, E: InteractiveElement>(
        element: E,
        player: impl Fn(&App) -> Option<Entity<VideoPlayer>> + 'static,
        run: fn(&mut VideoPlayer, &mut Context<VideoPlayer>),
    ) -> E {
        element.on_action(move |_: &A, _, cx| {
            if let Some(player) = player(cx) {
                player.update(cx, |player, cx| {
                    player.reveal(cx);
                    run(player, cx);
                    cx.notify();
                });
            }
        })
    }
    let element = forward::<TogglePlay, _>(element, player.clone(), |p, cx| p.toggle_play(cx));
    let element = forward::<SeekBackward, _>(element, player.clone(), |p, _| p.seek_by(-SKIP_STEP));
    let element = forward::<SeekForward, _>(element, player.clone(), |p, _| p.seek_by(SKIP_STEP));
    let element = forward::<StepFrameBackward, _>(element, player.clone(), |p, _| p.step_frame(-1));
    let element = forward::<StepFrameForward, _>(element, player.clone(), |p, _| p.step_frame(1));
    let element = forward::<VolumeUp, _>(element, player.clone(), |p, cx| {
        p.set_volume(p.playback.volume + VOLUME_STEP);
        p.show_hud(cx);
    });
    let element = forward::<VolumeDown, _>(element, player.clone(), |p, cx| {
        p.set_volume(p.playback.volume - VOLUME_STEP);
        p.show_hud(cx);
    });
    let element = forward::<ToggleMute, _>(element, player.clone(), |p, _| p.toggle_mute());
    let element = forward::<ToggleLoop, _>(element, player.clone(), |p, _| {
        p.playback.looping = !p.playback.looping
    });
    let element = forward::<RateIncrease, _>(element, player.clone(), |p, _| {
        p.set_rate(model::cycle_rate(p.playback.rate, 1))
    });
    let element = forward::<RateDecrease, _>(element, player.clone(), |p, _| {
        p.set_rate(model::cycle_rate(p.playback.rate, -1))
    });
    let element = forward::<RateReset, _>(element, player.clone(), |p, _| p.set_rate(1.));
    element.on_action(move |_: &ToggleFullscreen, window, cx| {
        let Some(player) = player(cx) else {
            return;
        };
        if player.read(cx).kind == Kind::Audio {
            // The player does not take F for audio; the viewer's own
            // binding of the key gets it.
            cx.propagate();
            return;
        }
        player.update(cx, |player, cx| player.reveal(cx));
        toggle_fullscreen(&player, window, cx);
    })
}

/// `reportPlaybackFailure`: one diagnostic line in production's shape.
fn report_playback_failure(source: &Path, message: &str, status: &Status) {
    let entry = serde_json::json!({
        // i18n: a JSON field
        "level": "ERROR",
        "source": "renderer",
        "target": "media.playback",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "message": serde_json::json!({
            "source": source.display().to_string(),
            "code": "MEDIA_ERR_DECODE",
            "message": message,
            "currentTime": status.position,
            "duration": status.duration,
            "paused": !status.playing,
        })
        .to_string(),
    });
    eprintln!("{entry}");
}

/// The scrubber's drag.
#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Seek,
}

/// A video's first picture has arrived.
pub struct FirstPicture;

impl EventEmitter<FirstPicture> for VideoPlayer {}

pub struct VideoPlayer {
    kind: Kind,
    path: PathBuf,
    backend: Option<Box<dyn Backend>>,
    /// AVFoundation refused the file and `ffmpeg` took over.
    fell_back: bool,
    playback: Playback,
    status: Status,
    reported: bool,
    frame: Option<Frame>,
    /// Since when the player has waited for its first picture. A video
    /// shows nothing until then (its host keeps what it showed before), so
    /// the first thing drawn is the video's own first frame rather than a
    /// thumbnail taken from further in.
    opened: Instant,
    controls: ControlsVisibility,
    controls_fade: motion::Fade,
    hide_task: Option<Task<()>>,
    drag: Option<Drag>,
    /// The scrubber's bounds, and where the pointer is over it (a
    /// fraction), while it is.
    bar: Bounds<Pixels>,
    bar_hover: Option<f32>,
    bar_fraction: f32,
    bar_expand: motion::Fade,
    rate_open: bool,
    /// The rate button's container and the open list, for closing the list
    /// on a press outside both.
    rate_bounds: Bounds<Pixels>,
    rate_menu_bounds: Bounds<Pixels>,
    /// When the volume display was last brought up.
    hud: Option<Instant>,
    hud_fade: motion::Fade,
    hud_visit: u64,
    analyser: visualizer::Analyser,
    /// The waveform the audio scrubber draws: unplayed and played.
    waveform: Option<(Arc<RenderImage>, Arc<RenderImage>)>,
    _waveform_task: Option<Task<()>>,
    /// Frames are polled until this time even while paused, so a seek's
    /// picture arrives.
    settle_until: Instant,
    /// Until when a picture is waited for after opening.
    first_picture_until: Instant,
}

impl VideoPlayer {
    /// Opens `path` and starts it as `MediaView` does: autoplay and loop from
    /// the options, sound muted for video. `waveform` is the audio item's
    /// thumbnail, which is its waveform.
    pub fn new(
        kind: Kind,
        path: PathBuf,
        mime: &str,
        options: Options,
        waveform: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let now = Instant::now();
        let opened = match kind {
            Kind::Video => backend::open_video(&path, mime),
            Kind::Audio => backend::open_audio(&path),
        };
        let mut playback = Playback::new(options);
        let (backend, status) = match opened {
            Ok(mut backend) => {
                backend.set_volume(playback.volume, playback.muted);
                backend.set_rate(playback.rate);
                if options.auto_play {
                    backend.play();
                    playback.playing = true;
                }
                (Some(backend), Status::default())
            }
            Err(error) => (
                None,
                Status {
                    error: Some(error),
                    ..Status::default()
                },
            ),
        };
        let waveform_task = waveform.filter(|_| kind == Kind::Audio).map(|thumbnail| {
            let load = cx
                .background_executor()
                .spawn(async move { waveform_layers(&thumbnail) });
            cx.spawn(async move |this, cx| {
                if let Some(layers) = load.await {
                    let _ = this.update(cx, |this: &mut Self, cx| {
                        this.waveform = Some(layers);
                        cx.notify();
                    });
                }
            })
        });
        let mut player = Self {
            kind,
            path,
            backend,
            fell_back: false,
            playback,
            status,
            reported: false,
            frame: None,
            opened: now,
            controls: ControlsVisibility::new(now),
            controls_fade: motion::Fade::at(true),
            hide_task: None,
            drag: None,
            bar: Bounds::default(),
            bar_hover: None,
            bar_fraction: 0.,
            bar_expand: motion::Fade::default(),
            rate_open: false,
            rate_bounds: Bounds::default(),
            rate_menu_bounds: Bounds::default(),
            hud: None,
            hud_fade: motion::Fade::default(),
            hud_visit: 0,
            analyser: visualizer::Analyser::default(),
            waveform: None,
            _waveform_task: waveform_task,
            settle_until: now,
            first_picture_until: now + Duration::from_secs(10),
        };
        player.schedule_hide(cx);
        player
    }

    /// Whether the player has something to show: a video its first picture
    /// (or a failure, or the wait has run out); audio at once.
    pub fn presentable(&self) -> bool {
        self.kind == Kind::Audio
            || self.frame.is_some()
            || self.backend.is_none()
            || self.status.error.is_some()
            || self.opened.elapsed() >= motion::MEDIA_HOLD
    }

    /// Stops at once, keeping the picture on screen.
    pub fn pause(&mut self) {
        if let Some(backend) = &mut self.backend {
            backend.pause();
        }
        self.playback.playing = false;
    }

    /// Whether a video's first picture is on screen.
    /// The picture shown now, for a video (`captureVideoFrame`); `None`
    /// for audio, which has none.
    pub fn capture_frame(&self) -> Option<Result<super::capture::Picture, String>> {
        if self.kind != Kind::Video {
            return None;
        }
        Some(match &self.frame {
            #[cfg(target_os = "macos")]
            Some(Frame::Surface(buffer)) => super::capture::from_pixel_buffer(buffer),
            Some(Frame::Image(image)) => super::capture::Picture::from_image(image),
            None => Err("The video frame is not ready yet.".into()),
        })
    }

    /// Whether this plays a video, whose frame can be captured.
    pub fn is_video(&self) -> bool {
        self.kind == Kind::Video
    }

    pub fn has_picture(&self) -> bool {
        self.frame.is_some()
    }

    /// Keeps taking pictures for a moment after a seek or a change of
    /// state, so the new picture arrives even while paused.
    fn settle(&mut self) {
        self.settle_until = Instant::now() + Duration::from_secs(1);
    }

    /// `revealControls`.
    fn reveal(&mut self, cx: &mut Context<Self>) {
        self.controls.reveal(Instant::now());
        self.controls_fade.set(true, motion::MEDIA_CONTROLS_FADE);
        self.schedule_hide(cx);
        cx.notify();
    }

    /// Wakes the player when the controls are due to hide.
    fn schedule_hide(&mut self, cx: &mut Context<Self>) {
        self.hide_task = self.controls.deadline().map(|deadline| {
            cx.spawn(async move |this, cx| {
                let wait = deadline.saturating_duration_since(Instant::now());
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |this: &mut Self, cx| {
                    if this.controls.tick(Instant::now()) {
                        this.controls_fade.set(false, motion::MEDIA_CONTROLS_FADE);
                        this.rate_open = false;
                        cx.notify();
                    }
                });
            })
        });
    }

    fn set_seeking(&mut self, seeking: bool, cx: &mut Context<Self>) {
        self.controls.set_hold(seeking, Instant::now());
        if seeking {
            self.controls_fade.set(true, motion::MEDIA_CONTROLS_FADE);
        }
        self.schedule_hide(cx);
    }

    /// `togglePlay`: playing from the end starts again from the beginning,
    /// as a media element does.
    fn toggle_play(&mut self, _: &mut Context<Self>) {
        let Some(backend) = &mut self.backend else {
            return;
        };
        if self.playback.playing {
            backend.pause();
            self.playback.playing = false;
        } else {
            if self.status.ended {
                backend.seek(0.);
                self.playback.seek_to(0., Instant::now());
            }
            backend.play();
            self.playback.playing = true;
        }
        self.settle();
    }

    fn seek(&mut self, target: f64) {
        let target = self.playback.seek_to(target, Instant::now());
        if let Some(backend) = &mut self.backend {
            backend.seek(target);
        }
        self.settle();
    }

    fn seek_by(&mut self, delta: f64) {
        self.seek(model::seek_relative(
            self.playback.current,
            delta,
            self.playback.duration,
        ));
    }

    /// `stepFrame`: pauses, then moves one frame.
    fn step_frame(&mut self, direction: i8) {
        if let Some(backend) = &mut self.backend {
            backend.pause();
        }
        self.playback.playing = false;
        let fps = self.status.fps.unwrap_or(model::DEFAULT_FPS);
        self.seek(model::step_frame(
            self.playback.current,
            direction,
            fps,
            self.playback.duration,
        ));
    }

    fn set_volume(&mut self, volume: f32) {
        self.playback.set_volume(volume);
        self.apply_volume();
    }

    fn toggle_mute(&mut self) {
        self.playback.muted = !self.playback.muted;
        self.apply_volume();
    }

    fn apply_volume(&mut self) {
        if let Some(backend) = &mut self.backend {
            backend.set_volume(self.playback.volume, self.playback.muted);
        }
    }

    fn set_rate(&mut self, rate: f32) {
        self.playback.rate = rate;
        if let Some(backend) = &mut self.backend {
            backend.set_rate(rate);
        }
    }

    /// `VolumeHUD`: shown for 800 ms after a volume key.
    fn show_hud(&mut self, cx: &mut Context<Self>) {
        let first = self.hud.is_none();
        self.hud = Some(Instant::now());
        if first {
            self.hud_fade = motion::Fade::default();
            self.hud_fade.set(true, motion::MEDIA_VOLUME_HUD_IN);
        }
        self.hud_visit += 1;
        let visit = self.hud_visit;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(VOLUME_HUD_DURATION).await;
            let _ = this.update(cx, |this: &mut Self, cx| {
                if this.hud_visit == visit {
                    this.hud = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Takes in what the media is doing, and the next picture.
    fn poll(&mut self, cx: &mut Context<Self>) {
        let Some(backend) = &mut self.backend else {
            return;
        };
        let status = backend.status();
        if let Some(error) = &status.error {
            // AVFoundation turns some QuickTime files down that ffmpeg plays.
            if backend.is_native() && self.frame.is_none() && !self.fell_back {
                self.fell_back = true;
                if let Ok(mut fallback) = backend::open_video_fallback(&self.path) {
                    fallback.set_volume(self.playback.volume, self.playback.muted);
                    fallback.set_rate(self.playback.rate);
                    if self.playback.playing {
                        fallback.play();
                    }
                    self.backend = Some(fallback);
                    self.settle();
                    return;
                }
            }
            if !self.reported {
                self.reported = true;
                report_playback_failure(&self.path, error, &status);
            }
        }
        let now = Instant::now();
        self.playback.duration = status.duration;
        self.playback.buffered = status.buffered;
        if self.drag != Some(Drag::Seek) {
            self.playback.report_position(status.position, now);
        }
        if status.ended && self.playback.playing {
            if self.playback.looping {
                backend.seek(0.);
                backend.play();
                self.playback.seek_to(0., now);
                self.settle_until = now + Duration::from_secs(1);
            } else {
                self.playback.playing = false;
            }
        } else if status.error.is_some() {
            self.playback.playing = false;
        }
        self.status = status;
        if let Some(frame) = backend.take_frame() {
            let first = self.frame.is_none();
            if let Some(Frame::Image(old)) = self.frame.replace(frame) {
                crate::atlas::release(old, cx);
            }
            if first {
                cx.emit(FirstPicture);
            }
        }
    }

    // ---- drawing ----

    fn picture(&self) -> Option<AnyElement> {
        match &self.frame {
            #[cfg(target_os = "macos")]
            Some(Frame::Surface(buffer)) => Some(
                surface(buffer.clone())
                    .object_fit(ObjectFit::Contain)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .into_any_element(),
            ),
            Some(Frame::Image(image)) => Some(
                img(image.clone())
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
            ),
            None => None,
        }
    }

    /// `ProgressBar`: the track, or the waveform for audio, with the time
    /// under the pointer.
    fn progress_bar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let progress = model::progress(self.playback.current, self.playback.duration);
        let buffered = model::progress(self.playback.buffered, self.playback.duration);
        let waveform = self.waveform.clone().filter(|_| self.kind == Kind::Audio);
        let height = if waveform.is_some() { 40. } else { 16. };
        let entity = cx.entity().downgrade();
        let mut bar = div()
            .id("media-progress")
            .relative()
            .w_full()
            .h(px(height))
            .flex()
            .items_center()
            .cursor_pointer()
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let _ = entity.update(cx, |this, cx| {
                            // The waveform is drawn at the bar's width: a new
                            // width needs another frame, which a notification
                            // made while drawing does not bring by itself.
                            let resized = this.bar.size != bounds.size;
                            this.bar = bounds;
                            if resized {
                                cx.notify();
                                window.request_animation_frame();
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        bar = match waveform {
            Some((base, played)) => {
                let width = f32::from(self.bar.size.width);
                let layer = |image: Arc<RenderImage>| {
                    img(image)
                        .absolute()
                        .left_0()
                        .top_0()
                        .w(px(width))
                        .h(px(40.))
                        .object_fit(ObjectFit::Fill)
                };
                bar.min_w(px(64.))
                    .child(layer(base))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .h_full()
                            .w(relative(progress))
                            .overflow_hidden()
                            .child(layer(played)),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(relative(progress))
                            .w(px(1.))
                            .bg(rgba(theme::MEDIA_FILL)),
                    )
            }
            None => {
                // `.progressTrack`, 4px tall, 6px while hovered or dragged.
                let expand = self.bar_expand.value_with(motion::EASE);
                let thickness = 4. + 2. * expand;
                let radius = thickness / 2.;
                let range = |fraction: f32, color: u32| {
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .h_full()
                        .w(relative(fraction))
                        .rounded(px(radius))
                        .bg(rgba(color))
                };
                bar.child(
                    div()
                        .relative()
                        .w_full()
                        .h(px(thickness))
                        .rounded(px(radius))
                        .overflow_hidden()
                        .bg(rgba(theme::MEDIA_TRACK))
                        .child(range(buffered, theme::MEDIA_FAINT))
                        .child(range(progress, theme::MEDIA_FILL)),
                )
            }
        };
        let hover = self
            .bar_hover
            .or_else(|| (self.drag == Some(Drag::Seek)).then_some(self.bar_fraction));
        if let Some(fraction) = hover.filter(|_| self.playback.duration > 0.) {
            // `.progressHoverTime`: centred over the pointer, its bottom 20px
            // above the bar's.
            bar = bar.child(
                div()
                    .absolute()
                    .bottom(px(20.))
                    .left(relative(fraction))
                    .w(px(0.))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .flex_none()
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(3.))
                            .bg(rgba(theme::MEDIA_POPUP))
                            .text_color(rgba(theme::MEDIA_FILL))
                            .font_family(theme::FONT_MONO)
                            .text(theme::TEXT_CAPTION)
                            .whitespace_nowrap()
                            .child(model::format_time(fraction as f64 * self.playback.duration)),
                    ),
            );
        }
        bar.on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            if !*hovered {
                this.bar_hover = None;
            }
            this.bar_expand.set(
                *hovered || this.drag == Some(Drag::Seek),
                motion::MEDIA_SCRUBBER_EXPAND,
            );
            cx.notify();
        }))
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
            this.bar_hover = Some(model::fraction_at(
                f32::from(event.position.x),
                f32::from(this.bar.origin.x),
                f32::from(this.bar.size.width),
            ));
            cx.notify();
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.reveal(cx);
                this.drag = Some(Drag::Seek);
                this.set_seeking(true, cx);
                this.scrub(event.position);
                this.bar_expand.set(true, motion::MEDIA_SCRUBBER_EXPAND);
                cx.notify();
            }),
        )
    }

    fn scrub(&mut self, position: Point<Pixels>) {
        self.bar_fraction = model::fraction_at(
            f32::from(position.x),
            f32::from(self.bar.origin.x),
            f32::from(self.bar.size.width),
        );
        self.seek(self.bar_fraction as f64 * self.playback.duration);
    }

    /// `VolumePanel`, setting this player's volume.
    fn volume_panel(&self, cx: &mut Context<Self>) -> controls::VolumePanel {
        let (change, mute) = (cx.entity().downgrade(), cx.entity().downgrade());
        controls::volume_panel("media-volume", self.playback.volume, self.playback.muted)
            .on_change(move |volume, _, cx| {
                let _ = change.update(cx, |this, cx| {
                    this.set_volume(volume);
                    cx.notify();
                });
            })
            .on_mute(move |_, cx| {
                let _ = mute.update(cx, |this, cx| {
                    this.toggle_mute();
                    cx.notify();
                });
            })
    }

    /// `PlaybackRateMenu`: the rate button and, while open, its list above.
    fn rate_menu(&self, cx: &mut Context<Self>) -> Div {
        let rate = self.playback.rate;
        let entity = cx.entity().downgrade();
        let mut container = div()
            .relative()
            .flex()
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = entity.update(cx, |this, _| this.rate_bounds = bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                label_button(
                    "media-rate",
                    model::rate_label(rate),
                    ui::tip(t("Playback speed")),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.rate_open = !this.rate_open;
                    cx.notify();
                })),
            );
        if self.rate_open {
            let entity = cx.entity().downgrade();
            let mut menu = div()
                .id("media-rate-menu")
                .occlude()
                .relative()
                .child(
                    canvas(
                        move |bounds, _, cx| {
                            let _ = entity.update(cx, |this, _| this.rate_menu_bounds = bounds);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .flex()
                .flex_col()
                .py(px(4.))
                .min_w(px(56.))
                .rounded(px(theme::RADIUS_MD))
                .bg(rgba(theme::MEDIA_MENU))
                .border_1()
                .border_color(rgba(theme::MEDIA_POPUP_BORDER));
            for choice in PLAYBACK_RATES {
                menu = menu.child(rate_row(choice, choice == rate).on_click(cx.listener(
                    move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.set_rate(choice);
                        this.rate_open = false;
                        cx.notify();
                    },
                )));
            }
            container = container.child(
                div()
                    .absolute()
                    .bottom(px(36.))
                    .left(relative(0.5))
                    .w(px(0.))
                    .flex()
                    .justify_center()
                    .child(menu),
            );
        }
        container
    }

    /// `VideoControls`, at opacity `shown`, taking the pointer while
    /// `visible`.
    fn controls(&self, shown: f32, visible: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let playing = self.playback.playing;
        let separator = div()
            .flex_none()
            .w(px(1.))
            .h(px(16.))
            .mx(px(4.))
            .bg(rgba(theme::MEDIA_FAINT));
        let time = div()
            .px(px(4.))
            .font_family(theme::FONT_MONO)
            .text(theme::TEXT_CAPTION)
            .text_color(rgba(theme::MEDIA_TEXT))
            .whitespace_nowrap()
            .child(format!(
                "{} / {}",
                model::format_time(self.playback.current),
                model::format_time(self.playback.duration)
            ));
        let skip = |seconds: f64| {
            if seconds < 0. {
                crate::tf!("Skip back {value0}s", SKIP_STEP)
            } else {
                crate::tf!("Skip forward {value0}s", SKIP_STEP)
            }
        };
        let left = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                media_button(
                    "media-play",
                    if playing {
                        "player-pause"
                    } else {
                        "player-play"
                    },
                    20.,
                    ui::tip(if playing { t("Pause") } else { t("Play") })
                        .shortcut("video.togglePlay", cx),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_play(cx);
                    cx.notify();
                })),
            )
            .child(
                media_button(
                    "media-back",
                    "player-skip-back",
                    18.,
                    ui::tip(skip(-1.)).shortcut("video.seekBackward", cx),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.seek((this.playback.current - SKIP_STEP).max(0.));
                    cx.notify();
                })),
            )
            .child(
                media_button(
                    "media-forward",
                    "player-skip-forward",
                    18.,
                    ui::tip(skip(1.)).shortcut("video.seekForward", cx),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.seek((this.playback.current + SKIP_STEP).min(this.playback.duration));
                    cx.notify();
                })),
            )
            .child(separator)
            .child(time);
        let looping = self.playback.looping;
        let mut right = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(self.rate_menu(cx))
            .child(
                media_button(
                    "media-loop",
                    if looping { "repeat" } else { "repeat-off" },
                    18.,
                    ui::tip(if looping { t("Loop on") } else { t("Loop off") })
                        .shortcut("video.toggleLoop", cx),
                )
                .toggled(looping)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.playback.looping = !this.playback.looping;
                    cx.notify();
                })),
            )
            .child(self.volume_panel(cx));
        if self.kind == Kind::Video {
            right = right.child(
                media_button(
                    "media-fullscreen",
                    "maximize",
                    18.,
                    ui::tip(t("Fullscreen")).shortcut("video.fullscreen", cx),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.reveal(cx);
                    toggle_fullscreen(&cx.entity(), window, cx);
                })),
            );
        }
        controls::controls_bar(shown, visible)
            .id("media-controls")
            .child(self.progress_bar(cx))
            .child(controls::button_row(left, right))
    }

    /// `VolumeHUD`: the glyph and percentage in a dark box at the middle.
    fn hud(&self) -> Option<Div> {
        self.hud?;
        let effective = self.playback.effective_volume();
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(4.))
                        .px(px(24.))
                        .py(px(16.))
                        .rounded(px(12.))
                        .bg(rgba(theme::MEDIA_HUD))
                        .text_color(rgba(theme::MEDIA_FILL))
                        .opacity(self.hud_fade.value_with(motion::EASE))
                        .child(ui::icon(
                            model::volume_icon(self.playback.volume, self.playback.muted),
                            28.,
                            rgba(theme::MEDIA_FILL),
                        ))
                        .child(
                            div()
                                .font_family(theme::FONT_MONO)
                                .text_size(px(theme::TEXT_XL))
                                // The player's inherited `line-height`.
                                .line_height(px(theme::TEXT_XL * theme::LINE_HEIGHT_BODY))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("{}%", (effective * 100.).round() as i32)),
                        ),
                ),
        )
    }
}

/// `.rateMenuItem`: a rate, highlighted under the pointer, bright when it is
/// the current one.
fn rate_row(rate: f32, current: bool) -> Stateful<Div> {
    let id = SharedString::from(format!("media-rate-{rate}"));
    let color = if current {
        theme::MEDIA_GLYPH_STRONG
    } else {
        theme::MEDIA_TEXT
    };
    div()
        .id(id)
        .w_full()
        .px(px(12.))
        .py(px(4.))
        .flex()
        .justify_center()
        .font_family(theme::FONT_MONO)
        .text(theme::TEXT_SM)
        .text_color(rgba(color))
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(|row| {
            row.bg(rgba(theme::MEDIA_POPUP_BORDER))
                .text_color(rgba(theme::MEDIA_GLYPH_STRONG))
        })
        .child(model::rate_label(rate))
}

/// The two looks of the audio scrubber's waveform, from the thumbnail's
/// alpha (`mask-image`): white at 20% for what is to come and 95% for what
/// has played. BGRA, as `RenderImage` holds pixels.
fn waveform_layers(path: &Path) -> Option<(Arc<RenderImage>, Arc<RenderImage>)> {
    let source = image::open(path).ok()?.to_rgba8();
    let layer = |alpha: f32| {
        let mut layer = source.clone();
        tint(&mut layer, alpha);
        Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
            layer
        )]))
    };
    Some((layer(0.2), layer(0.95)))
}

/// White at `alpha`, through the image's own alpha.
fn tint(image: &mut image::RgbaImage, alpha: f32) {
    for pixel in image.pixels_mut() {
        let coverage = pixel.0[3] as f32 / 255.;
        pixel.0 = [255, 255, 255, (alpha * coverage * 255.).round() as u8];
    }
}

impl Render for VideoPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.poll(cx);
        let now = Instant::now();
        if self.playback.playing
            || now < self.settle_until
            || (self.kind == Kind::Video && self.frame.is_none() && now < self.first_picture_until)
            // Paused before the media loaded: keep asking until its length
            // is known, so the time reads as production's does.
            || (self.playback.duration <= 0.
                && self.status.error.is_none()
                && now < self.first_picture_until)
            || self.controls_fade.animating()
            || self.bar_expand.animating()
            || self.hud_fade.animating()
        {
            window.request_animation_frame();
        }
        let video = self.kind == Kind::Video;
        if !self.presentable() {
            // Until the first picture: nothing, not even the black, so the
            // host's previous picture (or its background) stays.
            if self.opened.elapsed() < motion::MEDIA_HOLD {
                window.request_animation_frame();
            }
            return div().id("media-player").size_full();
        }
        let visible = self.controls.visible();
        let mut root = div()
            .id("media-player")
            .relative()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .bg(if video {
                rgba(theme::MEDIA_BACKGROUND)
            } else {
                theme::theme(cx).background()
            })
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| this.reveal(cx)))
            .on_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, _, cx| this.reveal(cx)))
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.toggle_play(cx);
                this.reveal(cx);
                // `onDoubleClick`: the second press of a double-click also
                // fullscreens a video (both presses have toggled playing).
                if video && event.click_count() == 2 {
                    toggle_fullscreen(&cx.entity(), window, cx);
                }
            }));
        if video {
            root = root.cursor_pointer().children(self.picture());
        } else {
            // `AudioVisualizer`: the analyser's numbers for this frame, drawn
            // under the controls.
            let mode = visualizer::Mode::from_setting(
                crate::settings::model(cx)
                    .read(cx)
                    .saved
                    .audio_visualization,
            );
            let data = match self.backend.as_ref().and_then(|b| b.analysis()) {
                Some(analysis) if mode != visualizer::Mode::None => {
                    self.analyser.read(mode, &analysis).to_vec()
                }
                _ => Vec::new(),
            };
            root = root.child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| visualizer::paint(mode, &data, bounds, window),
                )
                .absolute()
                .inset_0()
                .size_full(),
            );
        }
        // A drag of the scrubber follows the pointer anywhere in the window
        // until the button is up; an open rate menu
        // closes on a press anywhere outside it.
        let entity = cx.entity().downgrade();
        let (dragging, rate_open) = (self.drag.is_some(), self.rate_open);
        root = root.child(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    if dragging {
                        let player = entity.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = player.update(cx, |this, cx| {
                                    if this.drag == Some(Drag::Seek) {
                                        this.scrub(event.position);
                                    }
                                    cx.notify();
                                });
                            }
                        });
                        let player = entity.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = player.update(cx, |this, cx| {
                                    if this.drag.take() == Some(Drag::Seek) {
                                        this.set_seeking(false, cx);
                                        this.reveal(cx);
                                        let over = this.bar.contains(&event.position);
                                        this.bar_expand.set(over, motion::MEDIA_SCRUBBER_EXPAND);
                                    }
                                    cx.notify();
                                });
                            }
                        });
                    }
                    if rate_open {
                        let player = entity.clone();
                        window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = player.update(cx, |this, cx| {
                                    let inside = this.rate_bounds.contains(&event.position)
                                        || this.rate_menu_bounds.contains(&event.position);
                                    if !inside {
                                        this.rate_open = false;
                                        cx.notify();
                                    }
                                });
                            }
                        });
                    }
                },
            )
            .absolute()
            .size_full(),
        );
        // `.controlsHidden`: faded out and passing the pointer through.
        let fade = self.controls_fade.value_with(motion::EASE);
        if fade > 0. {
            let controls = self.controls(fade, visible, cx);
            root = root.child(if visible {
                controls
                    .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| this.reveal(cx)))
            } else {
                controls
            });
        }
        root.children(self.hud())
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        // Stop sound at once; the backend's threads wind down by themselves.
        if let Some(backend) = &mut self.backend {
            backend.pause();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, kind_of, tint};

    #[test]
    fn media_types_choose_the_player() {
        assert_eq!(kind_of("video/webm"), Some(Kind::Video));
        assert_eq!(kind_of("video/quicktime"), Some(Kind::Video));
        assert_eq!(kind_of("audio/mpeg"), Some(Kind::Audio));
        assert_eq!(kind_of("audio/wav"), Some(Kind::Audio));
        assert_eq!(kind_of("image/gif"), None);
    }

    #[test]
    fn the_waveform_takes_its_shape_from_the_thumbnail_alpha() {
        let mut image =
            image::RgbaImage::from_raw(2, 1, vec![0, 114, 239, 255, 0, 0, 0, 0]).unwrap();
        tint(&mut image, 0.2);
        assert_eq!(image.as_raw(), &vec![255, 255, 255, 51, 255, 255, 255, 0]);
    }
}
