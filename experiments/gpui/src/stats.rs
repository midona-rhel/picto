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
static GRID_TILES: AtomicU64 = AtomicU64::new(0);
/// Microseconds since `EPOCH` of the previous grid frame, for pacing.
static GRID_LAST_FRAME: AtomicU64 = AtomicU64::new(0);
static GRID_GAP_MICROS: AtomicU64 = AtomicU64::new(0);
static GRID_GAP_MAX_MICROS: AtomicU64 = AtomicU64::new(0);
static GRID_GAPS: AtomicU64 = AtomicU64::new(0);
/// Frames more than one and a half 60 Hz intervals after the one before.
static GRID_LATE_FRAMES: AtomicU64 = AtomicU64::new(0);
static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
static THUMBNAIL_READS: AtomicU64 = AtomicU64::new(0);
static THUMBNAIL_EVICTIONS: AtomicU64 = AtomicU64::new(0);

pub fn rendered(view: View) {
    RENDERS[view as usize].fetch_add(1, Relaxed);
}

/// The grid built one frame of `tiles` tiles, starting at `started`.
pub fn grid_built(started: Instant, tiles: usize) {
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

pub fn thumbnail_read() {
    THUMBNAIL_READS.fetch_add(1, Relaxed);
}

pub fn thumbnail_evicted() {
    THUMBNAIL_EVICTIONS.fetch_add(1, Relaxed);
}

pub fn start_reporting(cx: &mut App) {
    if std::env::var_os("PICTO_GPUI_STATS").is_none() {
        return;
    }
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let renders = RENDERS.each_ref().map(|count| count.swap(0, Relaxed));
            let grid = renders[View::Grid as usize];
            let build = GRID_BUILD_MICROS.swap(0, Relaxed);
            let longest = GRID_BUILD_MAX_MICROS.swap(0, Relaxed);
            let tiles = GRID_TILES.swap(0, Relaxed);
            let reads = THUMBNAIL_READS.swap(0, Relaxed);
            let evictions = THUMBNAIL_EVICTIONS.swap(0, Relaxed);
            let gaps = GRID_GAPS.swap(0, Relaxed);
            let gap = GRID_GAP_MICROS.swap(0, Relaxed);
            let widest = GRID_GAP_MAX_MICROS.swap(0, Relaxed);
            let late = GRID_LATE_FRAMES.swap(0, Relaxed);
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
                "stats repaints[{repaints}] grid-build avg={:.2}ms max={:.2}ms tiles/frame={} \
                 frame-gap avg={:.1}ms max={:.1}ms late={late}/{gaps} \
                 thumbnail-reads={reads} evictions={evictions}",
                build as f64 / grid.max(1) as f64 / 1000.,
                longest as f64 / 1000.,
                tiles / grid.max(1),
                gap as f64 / gaps.max(1) as f64 / 1000.,
                widest as f64 / 1000.,
            );
        }
    })
    .detach();
}
