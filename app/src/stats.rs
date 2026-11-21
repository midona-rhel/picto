//! Counts what the app does while it is used, when asked to: how often each
//! view is rendered, how long the grid takes to build a frame, the time
//! between one frame of the window and the next, and how many thumbnail files
//! are read. `meter.rs` records the end of each frame, and reports it to this
//! module too. Set `PICTO_GPUI_STATS=1` to print one line to standard error
//! for each second in which something happened. When the variable is not set,
//! the timings read no clock and each counter costs one atomic addition.
use gpui::App;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub enum View {
    Shell,
    Sidebar,
    Toolbar,
    Grid,
    Inspector,
    Viewer,
}

const VIEWS: [&str; 6] = ["shell", "sidebar", "toolbar", "grid", "inspector", "viewer"];

static RENDERS: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
static GRID_BUILD_MICROS: AtomicU64 = AtomicU64::new(0);
static GRID_BUILD_MAX_MICROS: AtomicU64 = AtomicU64::new(0);
/// The longest time the main thread itself ran to build one grid frame, in
/// microseconds. Time in which other processes made the thread wait is not
/// counted, so this is what the frame itself cost.
static GRID_BUILD_CPU_MAX_MICROS: AtomicU64 = AtomicU64::new(0);
static GRID_TILES: AtomicU64 = AtomicU64::new(0);
/// The end of the previous frame, in microseconds since `EPOCH`. The time
/// between two frames is measured from it.
static LAST_FRAME: AtomicU64 = AtomicU64::new(0);
static GAP_MICROS: AtomicU64 = AtomicU64::new(0);
static GAP_MAX_MICROS: AtomicU64 = AtomicU64::new(0);
static GAPS: AtomicU64 = AtomicU64::new(0);
/// The number of frames that ended more than 25 ms after the frame before,
/// which is one and a half frame intervals at 60 Hz.
static LATE_FRAMES: AtomicU64 = AtomicU64::new(0);
static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
/// The longest time one grid frame took to lay its window out again
/// (`GRID_LAYOUT_MAX_MICROS`) and to build the elements of its tiles
/// (`GRID_TILES_MAX_MICROS`), in microseconds.
static GRID_LAYOUT_MAX_MICROS: AtomicU64 = AtomicU64::new(0);
static GRID_TILES_MAX_MICROS: AtomicU64 = AtomicU64::new(0);
/// The longest time the main thread was busy before a short timer it had set
/// could run, in microseconds. Input and frames waited for that long behind
/// the thread's own work. A timer can also run late because the system let
/// the thread sleep longer, as it does for a window in the background, so
/// only the time the thread spent running in between is counted.
static STALL_MAX_MICROS: AtomicU64 = AtomicU64::new(0);
static THUMBNAIL_READS: AtomicU64 = AtomicU64::new(0);
static THUMBNAIL_EVICTIONS: AtomicU64 = AtomicU64::new(0);
/// The number of thumbnails that were decoded and released without ever being
/// painted.
static THUMBNAIL_UNSEEN: AtomicU64 = AtomicU64::new(0);
/// The number of thumbnail loads that were dropped before they finished,
/// because the tile had scrolled out of view.
static THUMBNAIL_ABANDONED: AtomicU64 = AtomicU64::new(0);
/// The largest number of bytes the decoded thumbnails held at one time.
static THUMBNAIL_BYTES_MAX: AtomicU64 = AtomicU64::new(0);
/// The number of pictures released through `atlas::release`
/// (`IMAGES_RELEASED`), and the number of those that a window's atlas still
/// held afterwards (`IMAGES_KEPT`), which is checked in test builds only.
static IMAGES_RELEASED: AtomicU64 = AtomicU64::new(0);
static IMAGES_KEPT: AtomicU64 = AtomicU64::new(0);

pub fn rendered(view: View) {
    RENDERS[view as usize].fetch_add(1, Relaxed);
}

/// Holds when a piece of work started: the time on the clock, and the time
/// the running thread had used until then. It holds nothing when the counters
/// are off.
pub struct Started(Option<(Instant, u64)>);

impl Started {
    pub fn now() -> Self {
        Self(enabled().then(|| (Instant::now(), thread_cpu_micros())))
    }

    fn cpu_micros(&self) -> Option<u64> {
        self.0
            .map(|(_, cpu)| thread_cpu_micros().saturating_sub(cpu))
    }
}

/// Returns how long the calling thread has run, in microseconds.
#[cfg(unix)]
fn thread_cpu_micros() -> u64 {
    #[repr(C)]
    struct Timespec {
        seconds: i64,
        nanoseconds: i64,
    }
    unsafe extern "C" {
        fn clock_gettime(clock: u32, time: *mut Timespec) -> i32;
    }
    #[cfg(target_os = "macos")]
    const CLOCK_THREAD_CPUTIME_ID: u32 = 16;
    #[cfg(not(target_os = "macos"))]
    const CLOCK_THREAD_CPUTIME_ID: u32 = 3;
    let mut time = Timespec {
        seconds: 0,
        nanoseconds: 0,
    };
    // SAFETY: `time` is a valid timespec for the call to fill.
    if unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut time) } != 0 {
        return 0;
    }
    time.seconds as u64 * 1_000_000 + time.nanoseconds as u64 / 1_000
}

/// Returns how long the calling thread has run, in microseconds. This is the
/// sum of its kernel time and its user time, which Windows counts in steps of
/// 100 nanoseconds.
#[cfg(windows)]
fn thread_cpu_micros() -> u64 {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};
    let (mut creation, mut exit, mut kernel, mut user) = Default::default();
    // SAFETY: the handle is the pseudo-handle of the current thread, and the
    // four FILETIMEs are valid for the call to fill.
    let read = unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    if read.is_err() {
        return 0;
    }
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    (ticks(kernel) + ticks(user)) / 10
}

#[cfg(not(any(unix, windows)))]
fn thread_cpu_micros() -> u64 {
    0
}

/// Records that the grid built one frame of `tiles` tiles, which it started
/// at `started`.
pub fn grid_built(started: Started, tiles: usize) {
    let (Some((wall, _)), Some(cpu)) = (started.0, started.cpu_micros()) else {
        return;
    };
    GRID_BUILD_CPU_MAX_MICROS.fetch_max(cpu, Relaxed);
    let micros = wall.elapsed().as_micros() as u64;
    GRID_BUILD_MICROS.fetch_add(micros, Relaxed);
    GRID_BUILD_MAX_MICROS.fetch_max(micros, Relaxed);
    GRID_TILES.fetch_add(tiles as u64, Relaxed);
}

/// Records that a frame of the window finished drawing at `ended` (`meter.rs`
/// calls this at the end of each frame). The time since the previous frame is
/// added to the counters only when it is under 100 ms, because frames that
/// close together belong to one continuous animation or scroll.
pub fn frame_drawn(ended: Instant) {
    if !enabled() {
        return;
    }
    let epoch = *EPOCH.get_or_init(Instant::now);
    let now = ended.saturating_duration_since(epoch).as_micros() as u64;
    let gap = now.saturating_sub(LAST_FRAME.swap(now, Relaxed));
    if gap < 100_000 {
        GAP_MICROS.fetch_add(gap, Relaxed);
        GAP_MAX_MICROS.fetch_max(gap, Relaxed);
        GAPS.fetch_add(1, Relaxed);
        if gap > 25_000 {
            LATE_FRAMES.fetch_add(1, Relaxed);
        }
    }
}

/// Names the part of a grid frame that [`grid_part`] times.
pub enum GridPart {
    /// Laying the grid's window of items out again, after the window, the
    /// width, the zoom or the mode changed.
    Layout,
    /// Building the elements of the visible tiles.
    Tiles,
}

/// Records that one part of building a grid frame, which started at
/// `started`, is done. The time counted is the time the main thread itself
/// ran.
pub fn grid_part(part: GridPart, started: Started) {
    let Some(micros) = started.cpu_micros() else {
        return;
    };
    match part {
        GridPart::Layout => GRID_LAYOUT_MAX_MICROS.fetch_max(micros, Relaxed),
        GridPart::Tiles => GRID_TILES_MAX_MICROS.fetch_max(micros, Relaxed),
    };
}

/// The time at which the app began preparing. The timings of results are
/// counted from it.
static LAUNCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
/// The time at which the latest window of results was asked for, in
/// microseconds since `LAUNCH`.
static WINDOW_ASKED: AtomicU64 = AtomicU64::new(0);

pub fn enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("PICTO_GPUI_STATS").is_some())
}

fn since_launch() -> Duration {
    LAUNCH.get_or_init(Instant::now).elapsed()
}

/// Prints that the window of results was read again in place after the
/// library changed.
pub fn window_refreshed(start: usize, count: usize) {
    if enabled() {
        eprintln!(
            "stats window-refreshed start={start} items={count} at={:.0}ms",
            since_launch().as_secs_f64() * 1000.
        );
    }
}

/// Prints at once that a stage of opening the library was reached (the
/// database can be read, the application finished opening), with the time
/// since launch.
pub fn opening(stage: &str) {
    if enabled() {
        eprintln!(
            "stats opening {stage} at={:.0}ms",
            since_launch().as_secs_f64() * 1000.
        );
    }
}

/// Prints at once that the library thread read a window of results starting
/// at `start`, and that the read took `took`. `what` says why it was read: a
/// destination was opened, the window moved, or the window was read again.
pub fn window_read(what: &str, start: usize, took: Duration) {
    if enabled() {
        eprintln!(
            "stats read {what} start={start} took={:.1}ms",
            took.as_secs_f64() * 1000.
        );
    }
}

/// Prints at once that a read or an edit of a screen ran on the library
/// thread for `took`, when that is 5 ms or more.
pub fn job_ran(took: Duration) {
    if enabled() && took >= Duration::from_millis(5) {
        eprintln!(
            "stats job took={:.1}ms at={:.0}ms",
            took.as_secs_f64() * 1000.,
            since_launch().as_secs_f64() * 1000.
        );
    }
}

/// Prints at once that the first window of a destination arrived, with the
/// time since launch.
pub fn results_opened(total: u64) {
    if enabled() {
        eprintln!(
            "stats results-opened total={total} at={:.0}ms",
            since_launch().as_secs_f64() * 1000.
        );
    }
}

/// Records the time at which another window of the results was asked for.
pub fn window_asked() {
    WINDOW_ASKED.store(since_launch().as_micros() as u64, Relaxed);
}

/// Prints at once that the window that was asked for arrived, with the time
/// it took.
pub fn window_arrived(start: usize, count: usize) {
    if enabled() {
        let took = since_launch()
            .as_micros()
            .saturating_sub(u128::from(WINDOW_ASKED.load(Relaxed)));
        eprintln!(
            "stats window start={start} items={count} took={:.1}ms at={:.0}ms",
            took as f64 / 1000.,
            since_launch().as_secs_f64() * 1000.
        );
    }
}

pub fn thumbnail_read() {
    THUMBNAIL_READS.fetch_add(1, Relaxed);
}

/// Records that a decoded thumbnail was released. `drawn` says whether it was
/// ever painted.
pub fn thumbnail_evicted(drawn: bool) {
    THUMBNAIL_EVICTIONS.fetch_add(1, Relaxed);
    if !drawn {
        THUMBNAIL_UNSEEN.fetch_add(1, Relaxed);
    }
}

pub fn thumbnail_abandoned() {
    THUMBNAIL_ABANDONED.fetch_add(1, Relaxed);
}

/// Records the number of bytes the decoded thumbnails hold now.
pub fn thumbnail_bytes(bytes: usize) {
    THUMBNAIL_BYTES_MAX.fetch_max(bytes as u64, Relaxed);
}

/// Records that a picture was released. `kept` says whether a window's atlas
/// still held it.
pub fn image_released(kept: bool) {
    IMAGES_RELEASED.fetch_add(1, Relaxed);
    if kept {
        IMAGES_KEPT.fetch_add(1, Relaxed);
    }
}

pub fn start_reporting(cx: &mut App) {
    since_launch();
    if !enabled() {
        return;
    }
    // A timer of a few milliseconds runs late when the main thread is busy
    // with other work. The lateness is counted only up to the time the thread
    // was running meanwhile.
    cx.spawn(async move |cx| {
        const TICK: Duration = Duration::from_millis(4);
        loop {
            let asked = Instant::now();
            let ran = thread_cpu_micros();
            cx.background_executor().timer(TICK).await;
            let late = asked.elapsed().saturating_sub(TICK).as_micros() as u64;
            let running = thread_cpu_micros().saturating_sub(ran);
            STALL_MAX_MICROS.fetch_max(late.min(running), Relaxed);
        }
    })
    .detach();
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let renders = RENDERS.each_ref().map(|count| count.swap(0, Relaxed));
            let grid = renders[View::Grid as usize];
            let build = GRID_BUILD_MICROS.swap(0, Relaxed);
            let longest = GRID_BUILD_MAX_MICROS.swap(0, Relaxed);
            let longest_cpu = GRID_BUILD_CPU_MAX_MICROS.swap(0, Relaxed);
            let tiles = GRID_TILES.swap(0, Relaxed);
            let reads = THUMBNAIL_READS.swap(0, Relaxed);
            let evictions = THUMBNAIL_EVICTIONS.swap(0, Relaxed);
            let unseen = THUMBNAIL_UNSEEN.swap(0, Relaxed);
            let abandoned = THUMBNAIL_ABANDONED.swap(0, Relaxed);
            let released = IMAGES_RELEASED.swap(0, Relaxed);
            let cached = THUMBNAIL_BYTES_MAX.swap(0, Relaxed);
            let kept = IMAGES_KEPT.swap(0, Relaxed);
            let gaps = GAPS.swap(0, Relaxed);
            let gap = GAP_MICROS.swap(0, Relaxed);
            let widest = GAP_MAX_MICROS.swap(0, Relaxed);
            let late = LATE_FRAMES.swap(0, Relaxed);
            let layout = GRID_LAYOUT_MAX_MICROS.swap(0, Relaxed);
            let tile_elements = GRID_TILES_MAX_MICROS.swap(0, Relaxed);
            let stall = STALL_MAX_MICROS.swap(0, Relaxed);
            if renders.iter().all(|count| *count == 0) && reads == 0 {
                continue;
            }
            let repaints = VIEWS
                .iter()
                .zip(renders)
                .filter(|(_, count)| *count > 0)
                .map(|(name, count)| format!("{name}={count}"))
                .collect::<Vec<_>>()
                .join(" ");
            eprintln!(
                "stats repaints[{repaints}] grid-build avg={:.2}ms max={:.2}ms \
                 (own time: max={:.2}ms layout max={:.2}ms tiles max={:.2}ms) tiles/frame={} \
                 frame-gap avg={:.1}ms max={:.1}ms late={late}/{gaps} \
                 main-thread stall max={:.1}ms thumbnail-reads={reads} evictions={evictions} \
                 (never painted={unseen}) abandoned={abandoned} thumbnails-held max={:.0}MB \
                 images-released={released} (still in an atlas={kept})",
                build as f64 / grid.max(1) as f64 / 1000.,
                longest as f64 / 1000.,
                longest_cpu as f64 / 1000.,
                layout as f64 / 1000.,
                tile_elements as f64 / 1000.,
                tiles / grid.max(1),
                gap as f64 / gaps.max(1) as f64 / 1000.,
                widest as f64 / 1000.,
                stall as f64 / 1000.,
                cached as f64 / 1_000_000.,
            );
        }
    })
    .detach();
}
