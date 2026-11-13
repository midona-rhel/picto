//! The player's arithmetic and state, with no drawing and no decoding:
//! `videoConstants.ts`, `videoTimeFormat.ts`, the actions of
//! `useVideoPlayer.ts`, the scrubber and volume track of `ProgressBar.tsx`
//! and `VolumePanel.tsx`, and the auto-hide of
//! `useMediaControlsVisibility.ts`.
use std::time::{Duration, Instant};

/// `PLAYBACK_RATES`.
pub const PLAYBACK_RATES: [f32; 6] = [0.25, 0.5, 0.75, 1., 2., 3.];
/// `CONTROLS_HIDE_DELAY`.
pub const CONTROLS_HIDE_DELAY: Duration = Duration::from_millis(2500);
/// `VOLUME_STEP`.
pub const VOLUME_STEP: f32 = 0.05;
/// `VOLUME_HUD_DURATION`.
pub const VOLUME_HUD_DURATION: Duration = Duration::from_millis(800);
/// `SKIP_STEP`, in seconds.
pub const SKIP_STEP: f64 = 5.;
/// `DEFAULT_FPS`.
pub const DEFAULT_FPS: f64 = 30.;
/// `initialVolume`'s default.
pub const INITIAL_VOLUME: f32 = 0.9;

/// How far the position a backend reports may be from a seek's target and
/// still count as having arrived, and how long the target is shown at most
/// while a seek is under way (the media element's `seeking` state).
const SEEK_ARRIVAL: f64 = 0.3;
const SEEK_PATIENCE: Duration = Duration::from_millis(1500);

/// `formatTime`: `m:ss`, or `h:mm:ss` from an hour; anything that is not a
/// finite, non-negative number shows as `0:00`.
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

/// `getFrac`: where `x` falls along a track from `left` that is `width`
/// wide, between 0 and 1.
pub fn fraction_at(x: f32, left: f32, width: f32) -> f32 {
    if width <= 0. {
        return 0.;
    }
    ((x - left) / width).clamp(0., 1.)
}

/// `getVol`: the volume at `y` on a vertical track from `top` that is
/// `height` tall; the top is full volume.
pub fn volume_at(y: f32, top: f32, height: f32) -> f32 {
    if height <= 0. {
        return 0.;
    }
    (1. - (y - top) / height).clamp(0., 1.)
}

/// How much of the media has played, as the scrubber fills it.
pub fn progress(current: f64, duration: f64) -> f32 {
    if duration > 0. {
        (current / duration).clamp(0., 1.) as f32
    } else {
        0.
    }
}

/// `seekRelative` and the skip buttons: a step from `current`, kept inside
/// the media.
pub fn seek_relative(current: f64, delta: f64, duration: f64) -> f64 {
    (current + delta).clamp(0., duration.max(0.))
}

/// `stepFrame`: one frame forward or back at the measured rate, or 30 fps.
pub fn step_frame(current: f64, direction: i8, fps: f64, duration: f64) -> f64 {
    let fps = if fps > 0. { fps } else { DEFAULT_FPS };
    seek_relative(current, direction as f64 / fps, duration)
}

/// `cyclePlaybackRate`: the next listed rate up or down, stopping at the
/// ends. A rate that is not listed moves to the first listed rate at or
/// above it, or the fastest.
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

/// `snapFps`: a measured frame rate, snapped to the common rate within 10%
/// of it, else rounded.
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

/// The rate button's label: `1x`, `0.25x`, `2x`.
pub fn rate_label(rate: f32) -> String {
    // JavaScript prints numbers without a trailing `.0`.
    let text = format!("{rate}");
    format!("{text}x")
}

/// The glyph the mute button and the volume HUD show.
pub fn volume_icon(volume: f32, muted: bool) -> &'static str {
    if muted || volume == 0. {
        "player-volume-off"
    } else if volume < 0.5 {
        "player-volume-2"
    } else {
        "player-volume"
    }
}

/// What the media is: the player's two looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Video,
    Audio,
}

/// The options a player opens with, from `MediaView.tsx`'s props.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub auto_play: bool,
    pub looping: bool,
    pub muted: bool,
    pub volume: f32,
    pub rate: f32,
}

impl Options {
    /// `MediaView.tsx` passes the Preview settings' autoplay and loop for
    /// video and leaves `muted` at the player's default, which is on. Audio
    /// gets the defaults (autoplay, loop) and starts with sound. Volume and
    /// speed start at 90% and 1× for every item; nothing is remembered.
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

/// `VideoPlayerState`, as the controls show it.
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
    /// A seek on its way: its target, shown as the time until the media has
    /// got there, and when it was asked for.
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

    /// `setVolume`: kept between 0 and 1; any sound unmutes.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0., 1.);
        if self.volume > 0. {
            self.muted = false;
        }
    }

    /// The volume the speakers get.
    pub fn effective_volume(&self) -> f32 {
        if self.muted { 0. } else { self.volume }
    }

    /// `seek`: the time shown moves to the target at once.
    pub fn seek_to(&mut self, target: f64, now: Instant) -> f64 {
        let target = target.clamp(0., self.duration.max(0.));
        self.current = target;
        self.seek = Some((target, now));
        target
    }

    /// Takes the position the media reports. While a seek is under way the
    /// target stays shown (`onTime` ignores updates while `seeking`), until
    /// the media reports a position near it or a moment has passed.
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

/// `useMediaControlsVisibility`: the controls show on any pointer movement
/// and hide 2.5 s after the last one, unless held open while the scrubber is
/// dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlsVisibility {
    visible: bool,
    hold: bool,
    hide_at: Option<Instant>,
}

impl ControlsVisibility {
    /// Shown, and counting down from the moment the player mounts, as the
    /// hook's first effect reveals them.
    pub fn new(now: Instant) -> Self {
        let mut visibility = Self {
            visible: true,
            hold: false,
            hide_at: None,
        };
        visibility.reveal(now);
        visibility
    }

    /// `reveal`.
    pub fn reveal(&mut self, now: Instant) {
        self.visible = true;
        self.hide_at = (!self.hold).then(|| now + CONTROLS_HIDE_DELAY);
    }

    /// `holdOpen` changed: held, they stay; released, they count down again.
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

    /// Hides them if their time has come; returns whether that changed.
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

    /// `controlsVisible`.
    pub fn visible(&self) -> bool {
        self.visible || self.hold
    }

    /// When they will hide, if they are counting down.
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
        // The top of the 80px track is full volume, the bottom silence.
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
        // Unlisted rates land on the first listed rate at or above them.
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
        // Once arrived, positions are taken as they come.
        playback.report_position(12.2, start + Duration::from_millis(100));
        assert_eq!(playback.current, 12.2);
        // A seek that never arrives gives way after a moment.
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
        // Movement brings them back and starts the count again.
        let moved = start + Duration::from_secs(3);
        controls.reveal(moved);
        assert!(controls.visible());
        assert_eq!(controls.deadline(), Some(moved + CONTROLS_HIDE_DELAY));
        // Dragging the scrubber holds them however long it takes.
        controls.set_hold(true, moved);
        assert_eq!(controls.deadline(), None);
        assert!(!controls.tick(moved + Duration::from_secs(60)));
        assert!(controls.visible());
        // Letting go counts down from then.
        let released = moved + Duration::from_secs(61);
        controls.set_hold(false, released);
        assert!(controls.visible());
        assert!(controls.tick(released + CONTROLS_HIDE_DELAY));
        assert!(!controls.visible());
    }
}
