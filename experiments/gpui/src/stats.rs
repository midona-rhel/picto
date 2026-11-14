//! Opt-in counters for what the app does while it is used: how often each
//! view repaints, how long the grid takes to build a frame, and how many
//! thumbnail files it reads. Set `PICTO_GPUI_STATS=1` to print one line per
//! active second to standard error. Costs a few atomic adds when off.
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
/// The same, counting only the time the main thread ran (not time it was
/// made to wait by other processes), which is the frame's own cost.
static GRID_BUILD_CPU_MAX_MICROS: AtomicU64 = AtomicU64::new(0);
static GRID_TILES: AtomicU64 = AtomicU64::new(0);
/// Microseconds since `EPOCH` of the previous grid frame, for pacing.
static GRID_LAST_FRAME: AtomicU64 = AtomicU64::new(0);
static GRID_GAP_MICROS: AtomicU64 = AtomicU64::new(0);
static GRID_GAP_MAX_MICROS: AtomicU64 = AtomicU64::new(0);
static GRID_GAPS: AtomicU64 = AtomicU64::new(0);
/// Frames more than one and a half 60 Hz intervals after the one before.
static GRID_LATE_FRAMES: AtomicU64 = AtomicU64::new(0);
static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
/// The longest a grid frame took laying its window out again, and building
/// its tiles' elements.
static GRID_LAYOUT_MAX_MICROS: AtomicU64 = AtomicU64::new(0);
static GRID_TILES_MAX_MICROS: AtomicU64 = AtomicU64::new(0);
/// The longest the main thread was busy before a short timer it had set
/// could run: how long input and frames waited behind its own work. A timer
/// can also run late because the system let it sleep longer (a window in
/// the background is put to rest); only the time the thread spent running
/// in between is counted.
static STALL_MAX_MICROS: AtomicU64 = AtomicU64::new(0);
static THUMBNAIL_READS: AtomicU64 = AtomicU64::new(0);
static THUMBNAIL_EVICTIONS: AtomicU64 = AtomicU64::new(0);
/// Thumbnails decoded and let go without ever being painted.
static THUMBNAIL_UNSEEN: AtomicU64 = AtomicU64::new(0);
/// Thumbnail loads dropped before they finished, the tile scrolled away.
static THUMBNAIL_ABANDONED: AtomicU64 = AtomicU64::new(0);
/// The most bytes the decoded thumbnails held at once.
static THUMBNAIL_BYTES_MAX: AtomicU64 = AtomicU64::new(0);
/// Pictures let go of through `atlas::release`, and of those, the ones a
/// window's atlas still held afterwards (checked in test builds only).
static IMAGES_RELEASED: AtomicU64 = AtomicU64::new(0);
static IMAGES_KEPT: AtomicU64 = AtomicU64::new(0);

pub fn rendered(view: View) {
    RENDERS[view as usize].fetch_add(1, Relaxed);
}

/// When a piece of work started, on the clock and in the running thread's
/// own time.
pub struct Started {
    wall: Instant,
    cpu: u64,
}

impl Started {
    pub fn now() -> Self {
        Self {
            wall: Instant::now(),
            cpu: thread_cpu_micros(),
        }
    }

    fn cpu_micros(&self) -> u64 {
        thread_cpu_micros().saturating_sub(self.cpu)
    }
}

/// How long the calling thread has run, in microseconds.
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

#[cfg(not(unix))]
fn thread_cpu_micros() -> u64 {
    0
}

/// The grid built one frame of `tiles` tiles, starting at `started`.
pub fn grid_built(started: Started, tiles: usize) {
    GRID_BUILD_CPU_MAX_MICROS.fetch_max(started.cpu_micros(), Relaxed);
    let started = started.wall;
    let micros = started.elapsed().as_micros() as u64;
    GRID_BUILD_MICROS.fetch_add(micros, Relaxed);
    GRID_BUILD_MAX_MICROS.fetch_max(micros, Relaxed);
    GRID_TILES.fetch_add(tiles as u64, Relaxed);
    // Pacing: the gap to the previous frame, while frames follow one another
    // closely enough to be one continuous animation or scroll.
    let now = EPOCH.get_or_init(Instant::now).elapsed().as_micros() as u64;
    let gap = now.saturating_sub(GRID_LAST_FRAME.swap(now, Relaxed));
    if gap < 100_000 {
        GRID_GAP_MICROS.fetch_add(gap, Relaxed);
        GRID_GAP_MAX_MICROS.fetch_max(gap, Relaxed);
        GRID_GAPS.fetch_add(1, Relaxed);
        if gap > 25_000 {
            GRID_LATE_FRAMES.fetch_add(1, Relaxed);
        }
    }
}

/// Which part of a grid frame [`grid_part`] times.
pub enum GridPart {
    /// Laying the window out again (another window, width, zoom, mode).
    Layout,
    /// Building the visible tiles' elements.
    Tiles,
}

/// A part of building one grid frame, started at `started`, is done. Its
/// cost is the main thread's own time.
pub fn grid_part(part: GridPart, started: Started) {
    let micros = started.cpu_micros();
    match part {
        GridPart::Layout => GRID_LAYOUT_MAX_MICROS.fetch_max(micros, Relaxed),
        GridPart::Tiles => GRID_TILES_MAX_MICROS.fetch_max(micros, Relaxed),
    };
}

/// When the app began preparing, which the result timings count from.
static LAUNCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
/// When the latest window of results was asked for, in microseconds since
/// `LAUNCH`.
static WINDOW_ASKED: AtomicU64 = AtomicU64::new(0);

fn enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("PICTO_GPUI_STATS").is_some())
}

fn since_launch() -> Duration {
    LAUNCH.get_or_init(Instant::now).elapsed()
}

/// The window was read again in place after the library changed.
pub fn window_refreshed(start: usize, count: usize) {
    if enabled() {
        eprintln!(
            "stats window-refreshed start={start} items={count} at={:.0}ms",
            since_launch().as_secs_f64() * 1000.
        );
    }
}

/// A stage of opening the library was reached (the database can be read,
/// the application finished opening): printed at once, with how long after
/// launch.
pub fn opening(stage: &str) {
    if enabled() {
        eprintln!(
            "stats opening {stage} at={:.0}ms",
            since_launch().as_secs_f64() * 1000.
        );
    }
}

/// The library thread read a window of results (`what`: a destination
/// opened, a window moved to, a window read again) starting at `start`, in
/// `took`: printed at once.
pub fn window_read(what: &str, start: usize, took: Duration) {
    if enabled() {
        eprintln!(
            "stats read {what} start={start} took={:.1}ms",
            took.as_secs_f64() * 1000.
        );
    }
}

/// A screen's own read or edit ran on the library thread for `took`:
/// printed at once when it took long enough to notice.
pub fn job_ran(took: Duration) {
    if enabled() && took >= Duration::from_millis(5) {
        eprintln!(
            "stats job took={:.1}ms at={:.0}ms",
            took.as_secs_f64() * 1000.,
            since_launch().as_secs_f64() * 1000.
        );
    }
}

/// A destination's first window arrived: printed at once, with how long
/// after launch.
pub fn results_opened(total: u64) {
    if enabled() {
        eprintln!(
            "stats results-opened total={total} at={:.0}ms",
            since_launch().as_secs_f64() * 1000.
        );
    }
}

/// Another window of the results was asked for.
pub fn window_asked() {
    WINDOW_ASKED.store(since_launch().as_micros() as u64, Relaxed);
}

/// The window asked for arrived: printed at once, with how long it took.
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

/// A decoded thumbnail was let go; `drawn` says whether it was ever painted.
pub fn thumbnail_evicted(drawn: bool) {
    THUMBNAIL_EVICTIONS.fetch_add(1, Relaxed);
    if !drawn {
        THUMBNAIL_UNSEEN.fetch_add(1, Relaxed);
    }
}

pub fn thumbnail_abandoned() {
    THUMBNAIL_ABANDONED.fetch_add(1, Relaxed);
}

/// The decoded thumbnails hold this many bytes now.
pub fn thumbnail_bytes(bytes: usize) {
    THUMBNAIL_BYTES_MAX.fetch_max(bytes as u64, Relaxed);
}

/// A picture was let go of; `kept` says a window's atlas still held it.
pub fn image_released(kept: bool) {
    IMAGES_RELEASED.fetch_add(1, Relaxed);
    if kept {
        IMAGES_KEPT.fetch_add(1, Relaxed);
    }
}

pub fn start_reporting(cx: &mut App) {
    since_launch();
    if std::env::var_os("PICTO_GPUI_STATS").is_none() {
        return;
    }
    // A timer of a few milliseconds that runs late ran behind work on the
    // main thread, as long as the thread was running meanwhile.
    cx.spawn(async move |cx| {
        const TICK: Duration = Duration::from_millis(4);
        loop {
            let asked = Started::now();
            cx.background_executor().timer(TICK).await;
            let late = asked.wall.elapsed().saturating_sub(TICK).as_micros() as u64;
            STALL_MAX_MICROS.fetch_max(late.min(asked.cpu_micros()), Relaxed);
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
            let gaps = GRID_GAPS.swap(0, Relaxed);
            let gap = GRID_GAP_MICROS.swap(0, Relaxed);
            let widest = GRID_GAP_MAX_MICROS.swap(0, Relaxed);
            let late = GRID_LATE_FRAMES.swap(0, Relaxed);
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
