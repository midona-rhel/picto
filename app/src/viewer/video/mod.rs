//! The media player, which plays the video and audio items that a viewer
//! shows.
//!
//! A video's picture fills the player's area. For audio, the audio
//! visualizer is drawn there in its place. The controls are drawn along the
//! bottom edge of that area. They are hidden 2.5 s after the last time the
//! pointer moved over the player, a mouse button was pressed on it or one of
//! its keys was used, and each of those shows them again. The controls are:
//!
//! - the progress bar, which shows how much of the media has played and how
//!   much is buffered, shows the time at the pointer's position, and is
//!   pressed or dragged to seek (for audio it shows the item's waveform);
//! - the play button and the two buttons that skip 5 s back and forward;
//! - the time, written as the position and the length;
//! - the button that opens the list of playback rates;
//! - the loop button;
//! - the mute button with the volume slider;
//! - for video only, the fullscreen button.
//!
//! This file also holds the volume display, which is shown in the centre of
//! the player for 800 ms after a volume key is pressed, the handlers of the
//! player's keyboard actions, and a video's fullscreen, in which the window
//! is full screen and shows only the player.
//!
//! The player reaches the media only through [`backend::Backend`]. A
//! backend decodes on its own threads, never on the thread that draws.
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

// The player's actions. `keymap.rs` binds keys to them in the scope
// `keymap::Scope::Media`, with three exceptions. `ExitFullscreen` is bound
// to Escape by `bind` in this file. `StepFrameBackward` and
// `StepFrameForward` have handlers in `on_actions`, but no key is bound to
// them.
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

/// The key context of the layer that shows the player over the whole of its
/// window (see `fullscreen_layer`). The Escape key that `bind` binds works
/// only in this context.
const FULLSCREEN_CONTEXT: &str = "PlayerFullscreen";

/// Binds Escape to `ExitFullscreen` in the key context `PlayerFullscreen`,
/// so that Escape ends the player's fullscreen. The layer that shows the
/// fullscreen player has that context and the keyboard focus. GPUI tries
/// the bindings of the innermost key context first, so this binding is
/// tried before any binding of Escape in the viewer or in the rest of the
/// window.
pub fn bind(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        ExitFullscreen,
        Some(FULLSCREEN_CONTEXT),
    )]);
}

/// The record of the one player that is fullscreen. It exists from
/// `enter_fullscreen` until `exit_fullscreen`. During that time the window
/// is full screen and `fullscreen_layer` draws the player over the whole
/// window, with its own controls and keys.
struct Fullscreen {
    /// The window that shows the player over its whole area.
    window: AnyWindowHandle,
    /// The player that is shown.
    player: WeakEntity<VideoPlayer>,
    /// The focus handle of the layer that shows the player. It has the
    /// keyboard focus while the player is fullscreen.
    focus: FocusHandle,
    /// The element that had the keyboard focus before the player became
    /// fullscreen. `exit_fullscreen` gives the focus back to it.
    return_focus: Option<FocusHandle>,
    /// True when the window was full screen already before the player
    /// became fullscreen, for example through View > Toggle Full Screen.
    /// `exit_fullscreen` then leaves the window full screen.
    was_full_screen: bool,
    /// True once the window has been full screen while the player was
    /// fullscreen. Only after that does a frame in which the window is not
    /// full screen end the player's fullscreen. So the player's fullscreen
    /// ends when the user takes the window out of full screen in another
    /// way, for example with the green button of a macOS window, but it
    /// does not end in the frames before the window has become full screen.
    seen_full_screen: bool,
    /// True after `release_fullscreen`, which a viewer calls when it stops
    /// showing the player because another item opens. `fullscreen_layer`
    /// ends the player's fullscreen the next time it runs.
    released: bool,
}

/// The app-wide state that holds the record of the player that is
/// fullscreen, or `None` when no player is.
#[derive(Default)]
struct FullscreenState(Option<Fullscreen>);

impl Global for FullscreenState {}

/// Returns true when `player` is the player that is shown over the whole of
/// its window, and `release_fullscreen` has not been called for it.
pub fn is_fullscreen(player: &Entity<VideoPlayer>, cx: &App) -> bool {
    cx.try_global::<FullscreenState>()
        .and_then(|state| state.0.as_ref())
        .is_some_and(|fullscreen| !fullscreen.released && fullscreen.player == player.downgrade())
}

/// Makes `player` fullscreen in `window`, or ends its fullscreen when it is
/// fullscreen already. It is called for a click on the player's fullscreen
/// button, for the `ToggleFullscreen` action (the F key unless the user has
/// changed the shortcut) and for a double-click on the picture. All three
/// callers call it only for a video's player, because audio has no
/// fullscreen.
pub fn toggle_fullscreen(player: &Entity<VideoPlayer>, window: &mut Window, cx: &mut App) {
    if is_fullscreen(player, cx) {
        exit_fullscreen(window, cx);
    } else {
        enter_fullscreen(player.downgrade(), window, cx);
    }
}

/// Makes `player` fullscreen in `window`. It records the element that has
/// the keyboard focus and whether the window is full screen already, makes
/// the window full screen if it is not, and gives the keyboard focus to the
/// layer that `fullscreen_layer` builds for the player.
fn enter_fullscreen(player: WeakEntity<VideoPlayer>, window: &mut Window, cx: &mut App) {
    // Only one player can be fullscreen at a time. If a player is
    // fullscreen already, in this window or in another one, its fullscreen
    // is ended first.
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

/// Ends the fullscreen of the player that is fullscreen, in whichever
/// window it is. The window leaves full screen, unless it was full screen
/// before the player became fullscreen. The keyboard focus goes back to
/// the element that had it before. The function does nothing when no player
/// is fullscreen.
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
    // `window` is the window that GPUI is updating now, and GPUI cannot
    // update a window through its handle while it is already updating it.
    // So when `window` is the window in which the player is fullscreen, the
    // changes are made on `window` itself. Otherwise they are made through
    // that other window's handle.
    if window.window_handle() == fullscreen.window {
        restore(window, cx);
    } else {
        let handle = fullscreen.window;
        let _ = handle.update(cx, |_, window, cx| restore(window, cx));
    }
}

/// Marks the fullscreen of `player` as released, if `player` is the player
/// that is fullscreen. A viewer calls this when it stops showing the player
/// because another item opens. From then on `is_fullscreen` returns false
/// for the player, and `fullscreen_layer` ends the fullscreen the next time
/// it runs for the window.
pub fn release_fullscreen(player: &Entity<VideoPlayer>, cx: &mut App) {
    if let Some(fullscreen) = cx.default_global::<FullscreenState>().0.as_mut()
        && fullscreen.player == player.downgrade()
    {
        fullscreen.released = true;
    }
}

/// Returns the layer that shows the fullscreen player over the whole of
/// `window`, or `None` when no player is fullscreen in `window`. The layer
/// has a black background, which shows in the parts of the window that the
/// picture does not cover. Nothing under the layer reacts to the pointer.
/// The layer has the key contexts `PlayerFullscreen` and `Media`, and it
/// handles `ExitFullscreen` and the player's other actions. The main window
/// (`shell/mod.rs`) and the detail window (`viewer/detail.rs`) add the
/// layer to their root element after their content, so that it is drawn
/// over the content.
///
/// The function also ends the fullscreen, and returns `None`, in three
/// cases: `release_fullscreen` was called for the player, the player no
/// longer exists, or the window has left full screen after it had been
/// full screen. `exit_fullscreen` then runs after the current update of
/// the window.
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

/// Returns which kind of media the media type `mime` is for the player:
/// `Kind::Video` for a type that starts with `video/`, and `Kind::Audio`
/// for a type that starts with `audio/`. Returns `None` for every other
/// type, which the player does not play.
pub fn kind_of(mime: &str) -> Option<Kind> {
    if mime.starts_with("video/") {
        Some(Kind::Video)
    } else if mime.starts_with("audio/") {
        Some(Kind::Audio)
    } else {
        None
    }
}

/// Returns the key context for a viewer's element: the context `base`, and
/// also the context `Media` when `showing` is true. A viewer passes true
/// while it shows the player. The player's shortcuts are bound in
/// `keymap::Scope::Media`, which requires the context `Media`, so they work
/// only while the player is shown.
pub fn key_context(base: &'static str, showing: bool) -> KeyContext {
    let mut context = KeyContext::default();
    context.add(base);
    if showing {
        context.add("Media"); // i18n: a key context
    }
    context
}

/// Adds a handler for each of the player's actions to `element`, which is
/// the element of a viewer that has the keyboard focus. `player` returns
/// the player that the viewer shows at that moment, or `None` when it shows
/// none. Each handler shows the player's controls (`reveal`), performs the
/// action on the player and asks for the player to be drawn again. A
/// handler does nothing when `player` returns `None`.
///
/// The actions do the following. `TogglePlay` plays or pauses.
/// `SeekBackward` and `SeekForward` move the position by 5 s (`SKIP_STEP`).
/// `StepFrameBackward` and `StepFrameForward` pause and move the position
/// by one frame. `VolumeUp` and `VolumeDown` change the volume by 0.05
/// (`VOLUME_STEP`) and show the volume display. `ToggleMute` and
/// `ToggleLoop` switch mute and loop. `RateIncrease` and `RateDecrease`
/// choose the next faster or slower rate in `PLAYBACK_RATES`, and
/// `RateReset` sets the rate to 1. `ToggleFullscreen` makes a video's
/// player fullscreen or ends its fullscreen. For an audio player its
/// handler does not show the controls and passes the action on.
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
            // Audio has no fullscreen, so the player does not handle the
            // action for an audio item. `cx.propagate()` tells GPUI that the
            // action was not handled. GPUI then tries the next binding that
            // matches the key that was pressed. So if the viewer binds the
            // same key to an action of its own, the key performs that
            // action.
            cx.propagate();
            return;
        }
        player.update(cx, |player, cx| player.reveal(cx));
        toggle_fullscreen(&player, window, cx);
    })
}

/// Writes one line of JSON to standard error which says that the media
/// file `source` could not be played. The line has five fields: the level
/// `ERROR`, the source `renderer`, the target `media.playback`, the time,
/// and `message`. `message` is a second JSON object, written as text. It
/// holds the file's path, the code `MEDIA_ERR_DECODE`, the backend's error
/// `message`, the position and the length in seconds, and whether playback
/// was paused. Picto 0.6.19 and earlier wrote the same entry for this
/// failure, with the same field names and the same fixed values.
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

/// A drag that the player follows across the whole window. The only one is
/// `Seek`: a drag of the progress bar, which lasts from a press of the left
/// button on the bar until the button is released. The volume slider
/// follows its own drag in `controls.rs`.
#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Seek,
}

/// The event a video's player emits when its first frame has arrived from
/// the backend. The viewer surface that shows the player
/// (`viewer/surface/load.rs`) then draws itself again, so that the player
/// replaces what the surface showed before.
pub struct FirstPicture;

impl EventEmitter<FirstPicture> for VideoPlayer {}

pub struct VideoPlayer {
    /// Whether the player plays a video or audio.
    kind: Kind,
    /// The media file. It is kept to open the file again with `ffmpeg` and
    /// to name the file in the report of a failure.
    path: PathBuf,
    /// The backend that decodes and plays the file, or `None` when the file
    /// could not be opened.
    backend: Option<Box<dyn Backend>>,
    /// True once AVFoundation has reported an error for the file before
    /// any frame arrived, and the player has tried to open the file with
    /// `ffmpeg` in its place. The player tries that only once.
    fell_back: bool,
    /// What the controls show: whether the media plays, the position, the
    /// length, the volume, mute, the rate and loop.
    playback: Playback,
    /// What the backend reported the last time it was asked (`poll`).
    status: Status,
    /// True once a failure to play the file has been written to standard
    /// error, so that it is written only once.
    reported: bool,
    /// The video frame that is drawn, or `None` before the first frame has
    /// arrived. It is always `None` for audio.
    frame: Option<Frame>,
    /// The moment the player was created. A video's player draws nothing
    /// until its first frame arrives, and the viewer keeps drawing what it
    /// showed before. That is the picture of the video's first frame that
    /// the library keeps, so the picture on screen does not change when the
    /// player draws its first frame. For a video that has no such picture
    /// yet it is the video's thumbnail, which is taken from a later point
    /// in the video (`viewer::picture_while_loading`). The player stops
    /// waiting 1,500 ms (`motion::MEDIA_HOLD`) after this moment (see
    /// `presentable`).
    opened: Instant,
    /// Whether the controls are shown, and when they are due to be hidden.
    controls: ControlsVisibility,
    /// The opacity of the controls: 1 while they are shown and 0 while
    /// they are hidden. It takes 300 ms (`motion::MEDIA_CONTROLS_FADE`) to
    /// change.
    controls_fade: motion::Fade,
    /// The timer that hides the controls when they are due to be hidden
    /// (see `schedule_hide`). It is `None` while the progress bar is
    /// dragged, when the controls stay shown.
    hide_task: Option<Task<()>>,
    /// The drag in progress, or `None` when there is none.
    drag: Option<Drag>,
    /// The position and size of the progress bar in window coordinates, in
    /// the frame that was drawn last.
    bar: Bounds<Pixels>,
    /// While the pointer is over the progress bar, the pointer's horizontal
    /// position on the bar as a fraction of the bar's width, from 0 at the
    /// left end to 1 at the right end. It is `None` while the pointer is
    /// not over the bar.
    bar_hover: Option<f32>,
    /// The same fraction for the last press on the progress bar or the
    /// last move of a drag of it. While the bar is dragged and `bar_hover`
    /// is `None`, the label with the time is drawn at this fraction.
    bar_fraction: f32,
    /// How thick the progress bar's track is drawn: 0 is 4 px and 1 is
    /// 6 px. It is 1 while the pointer is over the bar or the bar is
    /// dragged, and it takes 100 ms (`motion::MEDIA_SCRUBBER_EXPAND`) to
    /// change.
    bar_expand: motion::Fade,
    /// True while the list of playback rates is open.
    rate_open: bool,
    /// The position and size, in window coordinates, of the box around the
    /// rate button in the frame that was drawn last. A press of a mouse
    /// button outside both this and `rate_menu_bounds` closes the list of
    /// playback rates.
    rate_bounds: Bounds<Pixels>,
    /// The position and size, in window coordinates, of the open list of
    /// playback rates in the frame that was drawn last.
    rate_menu_bounds: Bounds<Pixels>,
    /// The moment a volume key last showed the volume display, or `None`
    /// while the display is not shown.
    hud: Option<Instant>,
    /// The opacity of the volume display. It goes from 0 to 1 in 150 ms
    /// (`motion::MEDIA_VOLUME_HUD_IN`) when the display appears. The
    /// display has no fade out.
    hud_fade: motion::Fade,
    /// Counts the times the volume display has been shown. Each time
    /// starts a timer of 800 ms, and only the timer of the latest time
    /// hides the display.
    hud_visit: u64,
    /// The state that the audio visualizer keeps from one frame to the
    /// next.
    analyser: visualizer::Analyser,
    /// The two pictures of the waveform that the progress bar of an audio
    /// item draws. The first is for the part that has not played yet, and
    /// the second is for the part that has played. It is `None` for video,
    /// and for audio until the pictures have been made.
    waveform: Option<(Arc<RenderImage>, Arc<RenderImage>)>,
    /// The task that makes the pictures of the waveform. GPUI cancels a
    /// task when it is dropped, so the player keeps the task for as long
    /// as the player exists.
    _waveform_task: Option<Task<()>>,
    /// Until this moment the player is drawn in every frame, also while
    /// playback is paused. Each time it is drawn it asks the backend for a
    /// new frame, so the frame for the position of a seek arrives. `settle`
    /// sets it to 1 s from now.
    settle_until: Instant,
    /// 10 s after the player was created. Until this moment the player is
    /// drawn in every frame while a video has no frame yet, and while the
    /// length of the media is not known and the backend has reported no
    /// error.
    first_picture_until: Instant,
    /// The timer that draws the player again while the media plays, at the
    /// moment when what the player shows next changes (see
    /// [`VideoPlayer::next_change`]).
    wake: Option<Task<()>>,
}

impl VideoPlayer {
    /// Makes a player for the file `path` and opens the file: a video with
    /// AVFoundation or `ffmpeg`, depending on `mime` and the platform
    /// (`backend::open_video`), and audio with `backend::open_audio`. The
    /// volume, mute, rate and loop are taken from `options`, and playback
    /// starts at once when `options.auto_play` is true. The viewer builds
    /// `options` with `Options::for_item`. When the file cannot be opened,
    /// the player has no backend and its status holds the error.
    ///
    /// `waveform` is the path of the item's thumbnail. For an audio item
    /// the thumbnail is a picture of the waveform, and the two pictures the
    /// progress bar draws are made from it on a background thread. It is
    /// not used for video.
    ///
    /// The controls are shown at first, and the timer that hides them
    /// after 2.5 s is started.
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
            wake: None,
        };
        player.schedule_hide(cx);
        player
    }

    /// Returns true when the viewer can show the player in place of what it
    /// showed before. An audio player can be shown at once. A video's
    /// player can be shown when its first frame has arrived, when the file
    /// could not be opened or the backend has reported an error, or when
    /// 1,500 ms (`motion::MEDIA_HOLD`) have passed since the player was
    /// created. Until then `render` draws nothing.
    pub fn presentable(&self) -> bool {
        self.kind == Kind::Audio
            || self.frame.is_some()
            || self.backend.is_none()
            || self.status.error.is_some()
            || self.opened.elapsed() >= motion::MEDIA_HOLD
    }

    /// Pauses playback. The video frame that was drawn last stays on
    /// screen.
    pub fn pause(&mut self) {
        if let Some(backend) = &mut self.backend {
            backend.pause();
        }
        self.playback.playing = false;
    }

    /// Returns the video frame that is drawn now as a picture, or an error
    /// message when no frame has arrived yet. Returns `None` for audio,
    /// which has no picture.
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

    /// Returns true when the player plays a video. The frame of a video
    /// can be captured with `capture_frame`.
    pub fn is_video(&self) -> bool {
        self.kind == Kind::Video
    }

    /// Returns true when the player has a video frame to draw, which is the
    /// case from the moment the first frame arrives.
    pub fn has_picture(&self) -> bool {
        self.frame.is_some()
    }

    /// Makes the player be drawn in every frame for the next second, by
    /// setting `settle_until`. Each time the player is drawn it asks the
    /// backend for a new frame, so the frame for a new position arrives
    /// even while playback is paused. It is called after a seek, after
    /// playback is started or paused with `toggle_play`, and after the
    /// player has changed to `ffmpeg`.
    fn settle(&mut self) {
        self.settle_until = Instant::now() + Duration::from_secs(1);
    }

    /// Shows the controls. They fade in over 300 ms if they were hidden,
    /// and the 2.5 s after which they are hidden are counted again from
    /// now.
    fn reveal(&mut self, cx: &mut Context<Self>) {
        self.controls.reveal(Instant::now());
        self.controls_fade.set(true, motion::MEDIA_CONTROLS_FADE);
        self.schedule_hide(cx);
        cx.notify();
    }

    /// Starts the timer that ends at the moment the controls are due to be
    /// hidden (`ControlsVisibility::deadline`), in place of the timer that
    /// was started before. When the timer ends and the controls are still
    /// due, they are hidden: they fade out over 300 ms, and the list of
    /// playback rates is closed. No timer is started while the controls
    /// have no such moment, which is the case while the progress bar is
    /// dragged.
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

    /// Called with true when a drag of the progress bar starts and with
    /// false when it ends. The controls stay shown during the drag. When
    /// the drag ends, the 2.5 s after which they are hidden are counted
    /// from that moment.
    fn set_seeking(&mut self, seeking: bool, cx: &mut Context<Self>) {
        self.controls.set_hold(seeking, Instant::now());
        if seeking {
            self.controls_fade.set(true, motion::MEDIA_CONTROLS_FADE);
        }
        self.schedule_hide(cx);
    }

    /// Pauses the media when it plays and plays it when it is paused. When
    /// the media has stopped at its end, playing starts again from the
    /// beginning. The function does nothing when the file could not be
    /// opened.
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

    /// Moves the position to `target`, in seconds, limited to the range
    /// from 0 to the length of the media. The controls show the new
    /// position at once, before the backend has reached it.
    fn seek(&mut self, target: f64) {
        let target = self.playback.seek_to(target, Instant::now());
        if let Some(backend) = &mut self.backend {
            backend.seek(target);
        }
        self.settle();
    }

    /// Moves the position by `delta` seconds from the position the controls
    /// show. A negative `delta` moves it back.
    fn seek_by(&mut self, delta: f64) {
        self.seek(model::seek_relative(
            self.playback.current,
            delta,
            self.playback.duration,
        ));
    }

    /// Pauses playback and moves the position by one frame: forward when
    /// `direction` is 1 and back when it is -1. The length of a frame comes
    /// from the frame rate the backend reported, or from 30 frames a second
    /// (`model::DEFAULT_FPS`) when it has reported none.
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

    /// Sets the volume to `volume`, limited to the range from 0 to 1. A
    /// volume above 0 also turns mute off (`Playback::set_volume`).
    fn set_volume(&mut self, volume: f32) {
        self.playback.set_volume(volume);
        self.apply_volume();
    }

    fn toggle_mute(&mut self) {
        self.playback.muted = !self.playback.muted;
        self.apply_volume();
    }

    /// Gives the backend the volume and the mute state that the controls
    /// show.
    fn apply_volume(&mut self) {
        if let Some(backend) = &mut self.backend {
            backend.set_volume(self.playback.volume, self.playback.muted);
        }
    }

    /// Sets the playback rate, where 1 is the normal speed.
    fn set_rate(&mut self, rate: f32) {
        self.playback.rate = rate;
        if let Some(backend) = &mut self.backend {
            backend.set_rate(rate);
        }
    }

    /// Shows the volume display and starts a timer that hides it after
    /// 800 ms (`VOLUME_HUD_DURATION`). It is called for each press of a
    /// volume key. A press while the display is shown keeps it shown for
    /// 800 ms from that press, because only the latest timer hides the
    /// display. The display fades in over 150 ms when it was not shown
    /// before.
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

    /// Asks the backend for its status and for a new video frame, and
    /// brings the player up to date with them. `render` calls it once each
    /// time the player is drawn. It does these things:
    ///
    /// - When the backend reports an error, and the backend is AVFoundation
    ///   and has delivered no frame yet, it opens the file with `ffmpeg` in
    ///   its place and returns. If that is not the case, or `ffmpeg` cannot
    ///   open the file either, it writes the failure to standard error,
    ///   once, and records that the media does not play.
    /// - It copies the length and the buffered range to `playback`. It
    ///   copies the position too, except while the progress bar is dragged,
    ///   when the position follows the pointer.
    /// - When the media has reached its end while playing, it starts the
    ///   media again from the beginning if loop is on, and records that the
    ///   media does not play if loop is off.
    /// - When the backend has a new frame, that frame replaces the one that
    ///   is drawn. The first frame makes the player emit `FirstPicture`.
    fn poll(&mut self, cx: &mut Context<Self>) {
        let Some(backend) = &mut self.backend else {
            return;
        };
        let status = backend.status();
        if let Some(error) = &status.error {
            // AVFoundation reports an error for some QuickTime files that
            // `ffmpeg` can play. When the backend is AVFoundation and it
            // has not delivered a frame yet, the file is opened again with
            // `ffmpeg`, with the same volume, mute, rate and playing state,
            // and that backend replaces AVFoundation. This is tried once
            // for a player.
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
            if first {
                // The time from the opening of the player to its first
                // frame is written to the log, so that a user's log shows
                // how long a video took to appear.
                log::info!(
                    target: "timing.first_frame",
                    "the first frame arrived {} ms after the player was opened",
                    self.opened.elapsed().as_millis()
                );
            }
            // A frame from `ffmpeg` is an image, and every window keeps an
            // image it has drawn in its sprite atlas until the image is
            // released. The frame that is replaced is released here, so
            // that the frames of the video do not stay in the atlas.
            if let Some(Frame::Image(old)) = self.frame.replace(frame) {
                crate::atlas::release(old, cx);
            }
            if first {
                cx.emit(FirstPicture);
            }
        }
    }

    /// Returns how long it takes, while the media plays, until what the
    /// player draws changes. `render` draws the player again after that
    /// time, instead of in every frame of the display.
    ///
    /// - For a video it is the time until the next frame of the video is
    ///   due, at the video's frame rate and the playback rate. It is `None`
    ///   while the backend has not reported a frame rate, and `render` then
    ///   draws the player in every frame. AVFoundation measures the frame
    ///   rate from the frames the player takes from it, so it reports none
    ///   at first.
    /// - For audio with a visualization it is 33 ms (`motion::LOOP_FRAME`).
    /// - For audio with the audio visualization setting "none", only the
    ///   time and the progress bar change. It is the time until the
    ///   position reaches the next whole second, or the time in which the
    ///   played part of the progress bar grows by 1 px, whichever is
    ///   shorter, and at least 33 ms.
    fn next_change(&self, cx: &App) -> Option<Duration> {
        let rate = f64::from(self.playback.rate.max(0.0625));
        if self.kind == Kind::Video {
            let fps = self.status.fps.filter(|fps| *fps > 0.)?;
            // `next` is the time in the video at which the frame after the
            // current one starts. The delay is the time until then at the
            // playback rate, plus 1 ms, so that the backend has that frame
            // ready when the player asks for it.
            let position = self.status.position.max(0.);
            let next = ((position * fps + 1e-6).floor() + 1.) / fps;
            return Some(Duration::from_secs_f64((next - position) / rate + 0.001));
        }
        let mode = visualizer::Mode::from_setting(
            &crate::settings::model(cx)
                .read(cx)
                .saved
                .audio_visualization,
        );
        if mode != visualizer::Mode::None {
            return Some(motion::LOOP_FRAME);
        }
        // Without a visualization only the time and the progress bar
        // change. `second` is the time until the position reaches the next
        // whole second, when the time that is shown changes. `pixel` is the
        // time in which the position moves by the length of media that
        // 1 px of the progress bar stands for. When the bar's width or the
        // media's length is not known yet, `second` is used for both.
        let position = self.playback.current.max(0.);
        let second = (position.floor() + 1. - position) / rate;
        let width = f64::from(f32::from(self.bar.size.width));
        let pixel = if width > 0. && self.playback.duration > 0. {
            self.playback.duration / width / rate
        } else {
            second
        };
        Some(Duration::from_secs_f64(
            second.min(pixel).max(motion::LOOP_FRAME.as_secs_f64()),
        ))
    }

    // The functions from here to the end of this `impl` block build the
    // elements that the player draws.

    /// Returns the element that draws the current video frame, or `None`
    /// when no frame has arrived yet. The frame is scaled to fit inside the
    /// player's area with its proportions kept (`ObjectFit::Contain`). On
    /// macOS a frame from AVFoundation is a CoreVideo buffer, which GPUI's
    /// `surface` element draws. A frame from `ffmpeg` is an image.
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

    /// Builds the progress bar, which shows the playback position and is
    /// pressed or dragged to seek. It fills the width of the controls,
    /// inside their padding.
    ///
    /// Without a waveform the bar is 16 px tall and shows a track in its
    /// vertical centre. The part of the track up to the end of the buffered
    /// range is white at 20%, and over it the part up to the playback
    /// position is white at 95%. With a waveform, which an audio item has
    /// once its pictures have been made, the bar is 40 px tall and shows
    /// the waveform in place of the track, with a vertical line 1 px wide
    /// at the playback position.
    ///
    /// While the pointer is over the bar or the bar is dragged, and the
    /// length of the media is known, a label above the bar shows the time
    /// at the pointer's position.
    ///
    /// A press of the left button on the bar seeks to the pointer's
    /// position and starts a drag. The handlers that follow the drag are
    /// registered in `render`.
    fn progress_bar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let progress = model::progress(self.playback.current, self.playback.duration);
        let buffered = model::progress(self.playback.buffered, self.playback.duration);
        let waveform = self.waveform.clone().filter(|_| self.kind == Kind::Audio);
        let height = if waveform.is_some() { 40. } else { 16. };
        let entity = cx.entity().downgrade();
        // Assistive technology sees the bar as a slider named "Seek". The
        // name is English in every language (`i18n::en`). The slider's
        // value is the playback position in whole seconds, from 0 to the
        // length of the media. Its increment and decrement actions seek
        // 5 s forward and back, as the seek keys do, and its set-value
        // action seeks to the given second.
        let stepped = |delta: f64| {
            let view = cx.entity().downgrade();
            move |_: Option<&gpui::accesskit::ActionData>, _: &mut Window, cx: &mut App| {
                let _ = view.update(cx, |this, cx| {
                    this.seek_by(delta);
                    cx.notify();
                });
            }
        };
        let set = cx.entity().downgrade();
        let mut bar = div()
            .id("media-progress")
            .role(Role::Slider)
            .aria_label(crate::i18n::en("Seek"))
            .aria_numeric_value(self.playback.current.round())
            .aria_min_numeric_value(0.)
            .aria_max_numeric_value(self.playback.duration.max(0.).round())
            .on_a11y_action(AccessibleAction::Increment, stepped(5.))
            .on_a11y_action(AccessibleAction::Decrement, stepped(-5.))
            .on_a11y_action(AccessibleAction::SetValue, move |data, _, cx| {
                if let Some(gpui::accesskit::ActionData::NumericValue(target)) = data {
                    let _ = set.update(cx, |this, cx| {
                        this.seek(*target);
                        cx.notify();
                    });
                }
            })
            .relative()
            .w_full()
            .h(px(height))
            .flex()
            .items_center()
            .cursor_pointer()
            .child(
                // The bar's position and size are recorded while each frame
                // is drawn. The handlers of a press and of a drag on the bar
                // read them to turn the pointer's position into a time.
                canvas(
                    move |bounds, _, cx| {
                        let _ = entity.update(cx, |this, _| this.bar = bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        bar = match waveform {
            Some((base, played)) => {
                // The waveform is two pictures of the same size, one in the
                // colour of the part still to play and one in the colour of
                // the part already played. The second is drawn over the
                // first and is cut off at the playback position. Both are
                // as wide as the bar, so they are built for the bar's
                // bounds in the frame that lays the bar out (`ui::sized`).
                let waveform = crate::ui::sized(move |bounds, _, _| {
                    let width = bounds.size.width;
                    let layer = |image: Arc<RenderImage>| {
                        img(image)
                            .absolute()
                            .left_0()
                            .top_0()
                            .w(width)
                            .h(px(40.))
                            .object_fit(ObjectFit::Fill)
                    };
                    div()
                        .relative()
                        .size_full()
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
                        .into_any_element()
                })
                .absolute()
                .inset_0();
                bar.min_w(px(64.))
                    .child(waveform)
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
                // The track is 4 px thick. While the pointer is over the
                // bar or the bar is dragged it is 6 px thick, and it
                // changes between the two in 100 ms (`bar_expand`). Its
                // ends are rounded with a radius of half its thickness. It
                // is white at 15%, and it clips the buffered part and the
                // played part to its own shape.
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
            // The label with the time at the pointer's position. Its
            // horizontal centre is at the pointer's position along the bar,
            // and its bottom edge is 20 px above the bar's bottom edge. It
            // is a box with a background of black at 80% and corners of
            // radius 3 px, and its text is 11 px, monospaced and white at
            // 95%. While the bar is dragged and the pointer is not over it,
            // the label is drawn at the position the drag has moved to
            // (`bar_fraction`), which stays between the bar's two ends.
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
        // The bar's handlers for the pointer. When the pointer comes onto
        // the bar or leaves it, the track starts to change its thickness,
        // and on leaving the pointer's position on the bar is cleared. A
        // move of the pointer over the bar records its position as a
        // fraction of the bar's width. A press of the left button shows
        // the controls, starts the drag, keeps the controls shown until the
        // drag ends, and seeks to the pointer's position. The press is not
        // passed on to the elements behind the bar.
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

    /// Seeks to the time that the progress bar shows at the horizontal
    /// position of `position`, which is the pointer's position in window
    /// coordinates. A position left of the bar seeks to the start of the
    /// media, and a position right of the bar seeks to its end. The
    /// position is also recorded in `bar_fraction`, as a fraction of the
    /// bar's width.
    fn scrub(&mut self, position: Point<Pixels>) {
        self.bar_fraction = model::fraction_at(
            f32::from(position.x),
            f32::from(self.bar.origin.x),
            f32::from(self.bar.size.width),
        );
        self.seek(self.bar_fraction as f64 * self.playback.duration);
    }

    /// Builds the volume control with `controls::volume_panel`: the mute
    /// button, and the volume slider that is shown above the button while
    /// the pointer is over either of them. A press or a drag on the slider
    /// sets this player's volume, and a click on the button turns this
    /// player's mute on or off.
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

    /// Builds the control for the playback rate. It is a button whose label
    /// is the rate in use, for example `1x`. A click on the button opens
    /// the list of the rates in `PLAYBACK_RATES`, and another click closes
    /// it. The list is drawn above the button, centred on it, with its
    /// bottom edge 36 px above the bottom edge of the button's box. A click
    /// on a row of the list sets that rate and closes the list.
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
            // The list extends above the controls, outside their bounds,
            // and it occludes what is behind it, so the handlers of the
            // player and of the controls do not see the pointer move over
            // the list. The list therefore has its own handler, which
            // shows the controls for each move of the pointer over it.
            // Without it the controls, and the list with them, would be
            // hidden 2.5 s after the pointer came onto the list, while the
            // user is choosing a rate.
            let mut menu = div()
                .id("media-rate-menu")
                .role(Role::Menu)
                .aria_label(t("Playback speed"))
                .occlude()
                .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| this.reveal(cx)))
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

    /// Builds the controls along the bottom edge of the player: the
    /// progress bar, and under it one row of buttons.
    ///
    /// At the left end of the row are the play or pause button, the button
    /// that skips 5 s back, the button that skips 5 s forward, a vertical
    /// line 1 px wide and 16 px tall, and the time, written as the
    /// position and the length, for example `0:05 / 1:30`. At the right
    /// end are the control for the playback rate, the loop button, the
    /// volume control and, for video only, the fullscreen button.
    ///
    /// `shown` is the opacity the controls are drawn with, from 0 to 1.
    /// While `visible` is true the controls occlude the player behind
    /// them, so a click on them does not also play or pause. While it is
    /// false, which is the case while they fade out, pointer events pass
    /// through them to the player.
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

    /// Builds the volume display, or returns `None` while it is not shown.
    /// It is a box in the centre of the player with a background of black
    /// at 70% and corners of radius 12 px. The box holds the volume glyph
    /// at 28 px and, under it, the volume as a percentage in 16 px semibold
    /// monospaced text. Both are white at 95%. The percentage is 0 while
    /// the player is muted.
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
                                // The percentage has the line height of
                                // body text, which is 1.55 times the font
                                // size (`LINE_HEIGHT_BODY`).
                                .line_height(px(theme::TEXT_XL * theme::LINE_HEIGHT_BODY))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("{}%", (effective * 100.).round() as i32)),
                        ),
                ),
        )
    }
}

/// Builds one row of the list of playback rates. The row shows the label
/// of `rate`, for example `0.75x`, centred, in 12 px monospaced text. The
/// text is white at 70%, or white at 95% when `current` is true, which the
/// caller passes for the rate in use. While the pointer is over the row,
/// the row's background is white at 10% and its text is white at 95%.
fn rate_row(rate: f32, current: bool) -> Stateful<Div> {
    let id = SharedString::from(format!("media-rate-{rate}"));
    let label = model::rate_label(rate);
    let color = if current {
        theme::MEDIA_GLYPH_STRONG
    } else {
        theme::MEDIA_TEXT
    };
    div()
        .id(id)
        .role(Role::MenuItemRadio)
        .aria_label(label.clone())
        .aria_toggled(crate::ui::a11y::toggled(current))
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
        .child(label)
}

/// Makes the two pictures of the waveform that the progress bar of an
/// audio item draws, from the item's thumbnail at `path`. Only the alpha
/// channel of the thumbnail is used, and the thumbnail's colours are
/// replaced with white. The first picture is white at 20% and is drawn for
/// the part that has not played yet. The second is white at 95% and is
/// drawn for the part that has played. Returns `None` when the thumbnail
/// cannot be read.
///
/// `RenderImage` holds its pixels in the order blue, green, red, alpha,
/// and the `image` crate in the order red, green, blue, alpha. Every pixel
/// here has the same value, 255, for red, green and blue, so the pixels
/// are the same in both orders and are not converted.
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

/// Replaces every pixel of `image` with white whose alpha is `alpha`
/// multiplied by the alpha the pixel had. A pixel that was fully opaque
/// becomes white at `alpha`, and a pixel that was fully transparent stays
/// fully transparent.
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
        // While the media plays, the player is not drawn in every frame of
        // the display. A timer draws it again after the delay that
        // `next_change` returns, which is the time until what the player
        // draws changes: the next frame of a video, the next step of the
        // audio visualization, or the next change of the time or of the
        // progress bar. Each time the player is drawn, the timer of the
        // time before is dropped and a new one is started.
        self.wake = None;
        let next = self
            .playback
            .playing
            .then(|| self.next_change(cx))
            .flatten();
        if let Some(delay) = next {
            self.wake = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                let _ = this.update(cx, |this, cx| {
                    this.wake = None;
                    cx.notify();
                });
            }));
        }
        // The player is drawn again in the next frame of the display in
        // these cases: the media plays and `next_change` returned no delay;
        // the moment `settle_until` has not come yet, which is 1 s after
        // the last call of `settle`, for example for a seek; a video has
        // no frame yet and less than 10 s have passed since the player was
        // created; or one of the three fades is in progress.
        if (self.playback.playing && next.is_none())
            || now < self.settle_until
            || (self.kind == Kind::Video && self.frame.is_none() && now < self.first_picture_until)
            // The player is also drawn again while the length of the media
            // is not known, the backend has reported no error and less than
            // 10 s have passed since the player was created. This covers
            // media that is paused before the backend has loaded it: the
            // player keeps asking the backend until the length is known,
            // so that the controls show the length.
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
            // While the player is not presentable (see `presentable`), which
            // is until a video's first frame arrives or 1,500 ms have
            // passed, it draws nothing at all, not even its black
            // background. What the viewer drew before the player, or the
            // viewer's background, stays visible in the player's area. The
            // player is drawn again in every frame during that time, and
            // each time it asks the backend for the first frame.
            if self.opened.elapsed() < motion::MEDIA_HOLD {
                window.request_animation_frame();
            }
            return div().id("media-player").size_full();
        }
        let visible = self.controls.visible();
        // The root element fills the player's area. Its background is black
        // for video and the theme's background colour for audio. A move of
        // the pointer over it, or a press of any mouse button on it, shows
        // the controls. A click on it plays or pauses, as the play button
        // does.
        let mut root = div()
            .id("media-player")
            .a11y_presentational()
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
                // The second click of a double-click on a video also makes
                // the player fullscreen, or ends its fullscreen. Each of
                // the two clicks has played or paused, so after a
                // double-click the media plays or is paused as it was
                // before.
                if video && event.click_count() == 2 {
                    toggle_fullscreen(&cx.entity(), window, cx);
                }
            }));
        if video {
            root = root.cursor_pointer().children(self.picture());
        } else {
            // The audio visualization is drawn over the whole of the
            // player's area, behind the controls. `mode` is the kind of
            // visualization chosen in the settings. `data` is what the
            // analyser computes for this frame from the samples the backend
            // is playing. It is empty when the mode is "none" and when the
            // backend has no samples, and nothing is drawn for the mode
            // "none".
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
        // This canvas draws nothing. It registers handlers for mouse events
        // anywhere in the window, for the frame that is being drawn.
        //
        // While the progress bar is dragged, a move of the pointer anywhere
        // in the window seeks to the pointer's position along the bar, and
        // the release of a mouse button ends the drag. The controls are
        // then shown and the 2.5 s after which they are hidden start again.
        // The track stays 6 px thick if the pointer is over the bar at the
        // release, and goes back to 4 px if it is not.
        //
        // While the list of playback rates is open, a press of a mouse
        // button anywhere outside the rate button's box and outside the
        // list closes the list.
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
        // The controls are built while their opacity (`fade`) is above 0,
        // so they are also drawn while they fade out. Once they are fully
        // hidden they are not built at all. While `visible` is true, the
        // controls occlude the root element, so the root's handler does
        // not see the pointer move over them. They get their own handler,
        // which shows the controls for each move of the pointer over them.
        // While they fade out (`visible` is false), pointer events pass
        // through them to the root.
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
        // The backend is paused first, so that the sound stops at once.
        // The backend's own threads and processes end after the backend is
        // dropped, and this function does not wait for them.
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
        // The image has two pixels: an opaque blue one and a fully
        // transparent black one. After `tint` with 0.2, the first is white
        // with alpha 51, which is 20% of 255, and the second is white with
        // alpha 0.
        let mut image =
            image::RgbaImage::from_raw(2, 1, vec![0, 114, 239, 255, 0, 0, 0, 0]).unwrap();
        tint(&mut image, 0.2);
        assert_eq!(image.as_raw(), &vec![255, 255, 255, 51, 255, 255, 255, 0]);
    }
}
