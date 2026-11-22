//! The media player's arithmetic and state. Nothing in this file draws or
//! decodes. It holds the player's constants, the way a time is written, the
//! arithmetic of the progress bar, of the volume slider, of seeking and of
//! the playback rates, the playback state that the controls show, and the
//! record of whether the controls are shown.
use std::time::{Duration, Instant};

/// The playback rates that the list of playback rates offers, from the
/// slowest to the fastest. 1 is the normal speed.
pub const PLAYBACK_RATES: [f32; 6] = [0.25, 0.5, 0.75, 1., 2., 3.];
/// How long the controls stay shown after the last time the pointer moved
/// over the player, a mouse button was pressed on it or one of its keys was
/// used.
pub const CONTROLS_HIDE_DELAY: Duration = Duration::from_millis(2500);
/// How much one press of a volume key changes the volume. The volume goes
/// from 0 to 1.
pub const VOLUME_STEP: f32 = 0.05;
/// How long the volume display stays shown after a volume key is pressed.
pub const VOLUME_HUD_DURATION: Duration = Duration::from_millis(800);
/// How far the two skip buttons and the two seek keys move the position, in
/// seconds.
pub const SKIP_STEP: f64 = 5.;
/// The frame rate, in frames a second, that a step of one frame assumes
/// when the backend has reported no frame rate.
pub const DEFAULT_FPS: f64 = 30.;
/// The volume every player starts with. 1 is the full volume.
pub const INITIAL_VOLUME: f32 = 0.9;

/// After a seek the controls keep showing the seek's target as the
/// position, until the backend reports a position that is at most this many
/// seconds from the target.
const SEEK_ARRIVAL: f64 = 0.3;
/// The longest time after a seek that the controls keep showing the seek's
/// target while the backend has not reported a position near it.
const SEEK_PATIENCE: Duration = Duration::from_millis(1500);

/// Returns `seconds` written as the controls show a time: `m:ss`, or
/// `h:mm:ss` when it is an hour or more. Returns `0:00` for a value that is
/// negative, infinite or not a number.
pub fn format_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0. {
        return "0:00".to_owned();
    }
    let total = seconds.floor() as u64;
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Returns where the horizontal position `x` is along a track that starts
/// at `left` and is `width` wide, as a fraction from 0 at the left end to 1
/// at the right end. A position outside the track gives 0 or 1, and a track
/// with no width gives 0.
pub fn fraction_at(x: f32, left: f32, width: f32) -> f32 {
    if width <= 0. {
        return 0.;
    }
    ((x - left) / width).clamp(0., 1.)
}

/// Returns the volume for the vertical position `y` on a track that starts
/// at `top` and is `height` tall: 1 at the top of the track and 0 at its
/// bottom. A position outside the track gives 1 or 0, and a track with no
/// height gives 0.
pub fn volume_at(y: f32, top: f32, height: f32) -> f32 {
    if height <= 0. {
        return 0.;
    }
    (1. - (y - top) / height).clamp(0., 1.)
}

/// Returns how much of the media has played, as a fraction from 0 to 1 of
/// its length. The progress bar draws its played part this wide. Returns 0
/// while the length is not known.
pub fn progress(current: f64, duration: f64) -> f32 {
    if duration > 0. {
        (current / duration).clamp(0., 1.) as f32
    } else {
        0.
    }
}

/// Returns the position `delta` seconds after `current`, limited to the
/// range from 0 to `duration`. A negative `delta` gives an earlier
/// position.
pub fn seek_relative(current: f64, delta: f64, duration: f64) -> f64 {
    (current + delta).clamp(0., duration.max(0.))
}

/// Returns the position one frame after `current` when `direction` is 1,
/// and one frame before it when `direction` is -1, limited to the range
/// from 0 to `duration`. A frame lasts `1 / fps` seconds. When `fps` is 0
/// or less, 30 frames a second (`DEFAULT_FPS`) are assumed.
pub fn step_frame(current: f64, direction: i8, fps: f64, duration: f64) -> f64 {
    let fps = if fps > 0. { fps } else { DEFAULT_FPS };
    seek_relative(current, direction as f64 / fps, duration)
}

/// Returns the rate next to `current` in `PLAYBACK_RATES`: the next faster
/// one when `direction` is 1 and the next slower one when it is -1. At
/// either end of the list it returns `current`. For a rate that is not in
/// the list it returns the first listed rate that is not below `current`,
/// or the fastest rate when there is none, whatever `direction` is.
pub fn cycle_rate(current: f32, direction: i8) -> f32 {
    let last = PLAYBACK_RATES.len() - 1;
    let next = match PLAYBACK_RATES.iter().position(|rate| *rate == current) {
        Some(index) => (index as isize + direction as isize).clamp(0, last as isize) as usize,
        None => PLAYBACK_RATES
            .iter()
            .position(|rate| *rate >= current)
            .unwrap_or(last),
    };
    PLAYBACK_RATES[next]
}

/// Returns the frame rate to use for the measured frame rate `measured`.
/// When the nearest of ten common frame rates (23.976, 24, 25, 29.97, 30,
/// 48, 50, 59.94, 60 and 120) differs from `measured` by less than 10% of
/// itself, that common rate is returned. Otherwise `measured` is returned,
/// rounded to a whole number. Only the AVFoundation backend measures its
/// frame rate. The `ffmpeg` backend reads the frame rate with `ffprobe`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn snap_fps(measured: f64) -> f64 {
    const COMMON: [f64; 10] = [23.976, 24., 25., 29.97, 30., 48., 50., 59.94, 60., 120.];
    let (best, distance) = COMMON
        .iter()
        .map(|rate| (*rate, (measured - rate).abs()))
        .fold((DEFAULT_FPS, f64::INFINITY), |best, candidate| {
            if candidate.1 < best.1 {
                candidate
            } else {
                best
            }
        });
    if distance / best < 0.1 {
        best
    } else {
        measured.round()
    }
}

/// Returns the label of the playback rate `rate`, which the rate button and
/// the rows of the list of playback rates show: `1x`, `0.25x`, `2x`.
pub fn rate_label(rate: f32) -> String {
    // `{rate}` writes a whole number without a decimal point, so the rate
    // 1 gives `1x` and not `1.0x`.
    let text = format!("{rate}");
    format!("{text}x")
}

/// Returns the name of the glyph that the mute button and the volume
/// display show: `player-volume-off` when the player is muted or the volume
/// is 0, `player-volume-2` when the volume is below 0.5, and
/// `player-volume` otherwise.
pub fn volume_icon(volume: f32, muted: bool) -> &'static str {
    if muted || volume == 0. {
        "player-volume-off"
    } else if volume < 0.5 {
        "player-volume-2"
    } else {
        "player-volume"
    }
}

/// The kind of media a player plays. A video's player draws the video's
/// picture, and an audio player draws the audio visualizer in its place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Video,
    Audio,
}

/// The settings a player starts with: whether playback starts at once,
/// whether the media loops, whether it is muted, the volume from 0 to 1,
/// and the playback rate, where 1 is the normal speed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub auto_play: bool,
    pub looping: bool,
    pub muted: bool,
    pub volume: f32,
    pub rate: f32,
}

impl Options {
    /// Returns the options for an item of the kind `kind`. A video starts
    /// playing at once when `video_auto_play` is true and loops when
    /// `video_loop` is true. The caller takes both from the settings. A
    /// video starts muted. Audio always starts playing at once, loops and
    /// is not muted. Every item starts at the volume 0.9 and the rate 1:
    /// the volume and the rate of the item shown before are not kept.
    pub fn for_item(kind: Kind, video_auto_play: bool, video_loop: bool) -> Self {
        match kind {
            Kind::Video => Self {
                auto_play: video_auto_play,
                looping: video_loop,
                muted: true,
                volume: INITIAL_VOLUME,
                rate: 1.,
            },
            Kind::Audio => Self {
                auto_play: true,
                looping: true,
                muted: false,
                volume: INITIAL_VOLUME,
                rate: 1.,
            },
        }
    }
}

/// The playback state that the controls show: whether the media plays, its
/// length and the position in seconds, the volume from 0 to 1, mute, the
/// playback rate and loop.
#[derive(Clone, Debug, PartialEq)]
pub struct Playback {
    pub playing: bool,
    pub duration: f64,
    pub current: f64,
    pub volume: f32,
    pub muted: bool,
    pub rate: f32,
    pub looping: bool,
    /// The end of the last buffered range, in seconds.
    pub buffered: f64,
    /// The seek that the backend has not reached yet: its target in
    /// seconds, and the moment it was asked for. `report_position` keeps
    /// the target as the position until the backend reports a position
    /// near it. It is `None` when there is no such seek.
    seek: Option<(f64, Instant)>,
}

impl Playback {
    pub fn new(options: Options) -> Self {
        Self {
            playing: false,
            duration: 0.,
            current: 0.,
            volume: options.volume.clamp(0., 1.),
            muted: options.muted,
            rate: options.rate,
            looping: options.looping,
            buffered: 0.,
            seek: None,
        }
    }

    /// Sets the volume to `volume`, limited to the range from 0 to 1. A
    /// volume above 0 also turns mute off.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0., 1.);
        if self.volume > 0. {
            self.muted = false;
        }
    }

    /// Returns the volume that is heard: 0 while muted, and the volume
    /// otherwise.
    pub fn effective_volume(&self) -> f32 {
        if self.muted { 0. } else { self.volume }
    }

    /// Records a seek to `target`, in seconds, that was asked for at `now`.
    /// The target is limited to the range from 0 to the length of the
    /// media, and it becomes the position that the controls show at once.
    /// Returns the limited target.
    pub fn seek_to(&mut self, target: f64, now: Instant) -> f64 {
        let target = target.clamp(0., self.duration.max(0.));
        self.current = target;
        self.seek = Some((target, now));
        target
    }

    /// Takes the position that the backend reports. After a seek the
    /// controls keep showing the seek's target: a reported position is
    /// ignored until it is at most 0.3 s from the target (`SEEK_ARRIVAL`)
    /// or 1.5 s have passed since the seek (`SEEK_PATIENCE`). From then on
    /// the reported position is the one that is shown.
    pub fn report_position(&mut self, position: f64, now: Instant) {
        if let Some((target, since)) = self.seek {
            let arrived = (position - target).abs() <= SEEK_ARRIVAL;
            if !arrived && now.duration_since(since) < SEEK_PATIENCE {
                return;
            }
            self.seek = None;
        }
        self.current = position;
    }
}

/// Records whether the player's controls are shown and when they are due
/// to be hidden. The controls are shown each time `reveal` is called, and
/// they are due to be hidden 2.5 s (`CONTROLS_HIDE_DELAY`) after the last
/// call. While they are held open (`set_hold`), which the player does
/// during a drag of the progress bar, they stay shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlsVisibility {
    visible: bool,
    hold: bool,
    hide_at: Option<Instant>,
}

impl ControlsVisibility {
    /// Returns the state for a new player: the controls are shown, and
    /// they are due to be hidden 2.5 s after `now`.
    pub fn new(now: Instant) -> Self {
        let mut visibility = Self {
            visible: true,
            hold: false,
            hide_at: None,
        };
        visibility.reveal(now);
        visibility
    }

    /// Shows the controls. They are due to be hidden 2.5 s after `now`,
    /// unless they are held open.
    pub fn reveal(&mut self, now: Instant) {
        self.visible = true;
        self.hide_at = (!self.hold).then(|| now + CONTROLS_HIDE_DELAY);
    }

    /// Holds the controls open when `hold` is true: they are shown and are
    /// not due to be hidden. When `hold` is false the hold ends, and the
    /// controls are due to be hidden 2.5 s after `now`. A call that does
    /// not change the hold does nothing.
    pub fn set_hold(&mut self, hold: bool, now: Instant) {
        if self.hold == hold {
            return;
        }
        self.hold = hold;
        if hold {
            self.hide_at = None;
            self.visible = true;
        } else {
            self.reveal(now);
        }
    }

    /// Hides the controls when the moment they are due to be hidden has
    /// come. Returns true when the controls were shown before the call and
    /// the call hid them.
    pub fn tick(&mut self, now: Instant) -> bool {
        match self.hide_at {
            Some(at) if now >= at => {
                self.hide_at = None;
                let changed = self.visible;
                self.visible = false;
                changed
            }
            _ => false,
        }
    }

    /// Returns true while the controls are shown or held open.
    pub fn visible(&self) -> bool {
        self.visible || self.hold
    }

    /// Returns the moment the controls are due to be hidden. Returns
    /// `None` when they are hidden already or held open.
    pub fn deadline(&self) -> Option<Instant> {
        self.hide_at
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CONTROLS_HIDE_DELAY, ControlsVisibility, Kind, Options, Playback, cycle_rate, format_time,
        fraction_at, progress, rate_label, seek_relative, snap_fps, step_frame, volume_at,
        volume_icon,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn time_formats_as_the_player_writes_it() {
        assert_eq!(format_time(0.), "0:00");
        assert_eq!(format_time(5.99), "0:05");
        assert_eq!(format_time(65.), "1:05");
        assert_eq!(format_time(600.), "10:00");
        assert_eq!(format_time(3599.9), "59:59");
        assert_eq!(format_time(3600.), "1:00:00");
        assert_eq!(format_time(3725.), "1:02:05");
        assert_eq!(format_time(-1.), "0:00");
        assert_eq!(format_time(f64::NAN), "0:00");
        assert_eq!(format_time(f64::INFINITY), "0:00");
    }

    #[test]
    fn scrubber_and_volume_track_arithmetic() {
        assert_eq!(fraction_at(150., 100., 200.), 0.25);
        assert_eq!(fraction_at(50., 100., 200.), 0.);
        assert_eq!(fraction_at(400., 100., 200.), 1.);
        assert_eq!(fraction_at(10., 0., 0.), 0.);
        // The track starts at 10 and is 80 tall. Its top is the full volume
        // and its bottom is the volume 0.
        assert_eq!(volume_at(10., 10., 80.), 1.);
        assert_eq!(volume_at(90., 10., 80.), 0.);
        assert_eq!(volume_at(30., 10., 80.), 0.75);
        assert_eq!(volume_at(200., 10., 80.), 0.);
        assert_eq!(progress(5., 10.), 0.5);
        assert_eq!(progress(15., 10.), 1.);
        assert_eq!(progress(5., 0.), 0.);
    }

    #[test]
    fn seeks_and_frame_steps_stay_inside_the_media() {
        assert_eq!(seek_relative(3., -5., 10.), 0.);
        assert_eq!(seek_relative(8., 5., 10.), 10.);
        assert_eq!(seek_relative(4., 5., 10.), 9.);
        assert_eq!(seek_relative(4., 5., 0.), 0.);
        assert!((step_frame(1., 1, 25., 10.) - 1.04).abs() < 1e-9);
        assert!((step_frame(1., -1, 0., 10.) - (1. - 1. / 30.)).abs() < 1e-9);
    }

    #[test]
    fn rates_step_through_the_list_and_stop_at_its_ends() {
        assert_eq!(cycle_rate(1., 1), 2.);
        assert_eq!(cycle_rate(1., -1), 0.75);
        assert_eq!(cycle_rate(3., 1), 3.);
        assert_eq!(cycle_rate(0.25, -1), 0.25);
        // 1.5 and 4 are not in the list. Each moves to the first listed
        // rate that is not below it, or to the fastest rate.
        assert_eq!(cycle_rate(1.5, 1), 2.);
        assert_eq!(cycle_rate(1.5, -1), 2.);
        assert_eq!(cycle_rate(4., 1), 3.);
        assert_eq!(rate_label(1.), "1x");
        assert_eq!(rate_label(0.25), "0.25x");
        assert_eq!(rate_label(2.), "2x");
    }

    #[test]
    fn measured_frame_rates_snap_to_common_ones() {
        assert_eq!(snap_fps(23.97), 23.976);
        assert_eq!(snap_fps(29.5), 29.97);
        assert_eq!(snap_fps(59.), 59.94);
        assert_eq!(snap_fps(15.2), 15.);
        assert_eq!(snap_fps(200.), 200.);
    }

    #[test]
    fn volume_changes_unmute_and_choose_the_glyph() {
        let mut playback = Playback::new(Options::for_item(Kind::Video, true, true));
        assert!(playback.muted);
        assert_eq!(playback.effective_volume(), 0.);
        playback.set_volume(playback.volume + 0.05);
        assert!(!playback.muted);
        assert!((playback.volume - 0.95).abs() < 1e-6);
        playback.set_volume(1.2);
        assert_eq!(playback.volume, 1.);
        playback.muted = true;
        playback.set_volume(0.);
        assert!(playback.muted, "silence does not unmute");
        assert_eq!(volume_icon(0.8, false), "player-volume");
        assert_eq!(volume_icon(0.3, false), "player-volume-2");
        assert_eq!(volume_icon(0.8, true), "player-volume-off");
        assert_eq!(volume_icon(0., false), "player-volume-off");
    }

    #[test]
    fn options_follow_media_view() {
        let video = Options::for_item(Kind::Video, false, false);
        assert!(!video.auto_play && !video.looping && video.muted);
        let audio = Options::for_item(Kind::Audio, false, false);
        assert!(audio.auto_play && audio.looping && !audio.muted);
        assert_eq!(audio.volume, 0.9);
    }

    #[test]
    fn a_seek_shows_its_target_until_the_media_arrives() {
        let start = Instant::now();
        let mut playback = Playback::new(Options::for_item(Kind::Video, true, true));
        playback.duration = 20.;
        assert_eq!(playback.seek_to(25., start), 20.);
        playback.seek_to(12., start);
        playback.report_position(3., start + Duration::from_millis(50));
        assert_eq!(playback.current, 12.);
        playback.report_position(12.1, start + Duration::from_millis(80));
        assert_eq!(playback.current, 12.1);
        // After the backend has reached the target, every reported position
        // is shown.
        playback.report_position(12.2, start + Duration::from_millis(100));
        assert_eq!(playback.current, 12.2);
        // When the backend does not reach the target of a seek, the
        // reported position is shown again 1.5 s after the seek.
        playback.seek_to(5., start);
        playback.report_position(9., start + Duration::from_secs(2));
        assert_eq!(playback.current, 9.);
    }

    #[test]
    fn controls_hide_after_the_delay_unless_held() {
        let start = Instant::now();
        let mut controls = ControlsVisibility::new(start);
        assert!(controls.visible());
        assert!(!controls.tick(start + Duration::from_millis(2499)));
        assert!(controls.visible());
        assert!(controls.tick(start + CONTROLS_HIDE_DELAY));
        assert!(!controls.visible());
        // `reveal` shows the controls again, and the 2.5 s are counted from
        // that moment.
        let moved = start + Duration::from_secs(3);
        controls.reveal(moved);
        assert!(controls.visible());
        assert_eq!(controls.deadline(), Some(moved + CONTROLS_HIDE_DELAY));
        // While the controls are held open they stay shown, however much
        // time passes.
        controls.set_hold(true, moved);
        assert_eq!(controls.deadline(), None);
        assert!(!controls.tick(moved + Duration::from_secs(60)));
        assert!(controls.visible());
        // When the hold ends, the 2.5 s are counted from that moment.
        let released = moved + Duration::from_secs(61);
        controls.set_hold(false, released);
        assert!(controls.visible());
        assert!(controls.tick(released + CONTROLS_HIDE_DELAY));
        assert!(!controls.visible());
    }
}
