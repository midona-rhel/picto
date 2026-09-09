//! Requested-allocation upper bound for this synthetic native workload. Counts
//! all Rust allocations (including shared snapshots exactly once) plus SQLite's
//! own allocation peak. Not renderer/decoded-image memory or allocator overhead.
use picto_library::predicate::SortField;
use picto_library::query::{ItemScope, RootQuery, WindowRequest};
use picto_library::{Library, RootId};
use serde_json::json;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Counted;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
fn add(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
}
// SAFETY: every operation delegates the original pointer/layout to System.
// Counters only observe successful allocations and never allocate recursively.
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = System.alloc(layout);
        if !pointer.is_null() {
            add(layout.size());
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = System.alloc_zeroed(layout);
        if !pointer.is_null() {
            add(layout.size());
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let next = System.realloc(pointer, layout, size);
        if !next.is_null() {
            if size >= layout.size() {
                add(size - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - size, Ordering::Relaxed);
            }
        }
        next
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;

fn record(stage: &str, library: &Library, retained: usize) {
    // SAFETY: SQLite's thread-safe process-wide diagnostic functions take no
    // pointers. Passing zero preserves the cumulative high-water mark.
    let (sqlite_live, sqlite_peak) = unsafe {
        (
            rusqlite::ffi::sqlite3_memory_used(),
            rusqlite::ffi::sqlite3_memory_highwater(0),
        )
    };
    let rust_live = LIVE.load(Ordering::Relaxed);
    let rust_peak = PEAK.load(Ordering::Relaxed);
    println!(
        "MEMORY {}",
        json!({"stage":stage,"retained_snapshots":retained,
        "rust_live_requested_bytes":rust_live,"rust_peak_requested_bytes":rust_peak,
        "sqlite_live_bytes":sqlite_live,"sqlite_peak_bytes":sqlite_peak,
        "conservative_sum_of_peaks_bytes":rust_peak as u64+sqlite_peak as u64,
        "within_1_gib_requested_allocation_bound":rust_peak as u64+sqlite_peak as u64<=1024*1024*1024,
        "query":library.query_statistics()})
    );
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("Expected isolated synthetic SQLite copy")?;
    let library = Arc::new(Library::open(path)?);
    assert_eq!(library.projections().snapshot().active().len(), 1_000_000);
    record("opened", &library, 0);
    let mut query = RootQuery {
        scope: ItemScope::All,
        view: Default::default(),
    };
    for start in (0..1_000_000).step_by(1500) {
        let window = library.query_window(&query, &WindowRequest::at(start, 1500))?;
        assert_eq!(window.page.total, 1_000_000);
    }
    record("full_traversal", &library, 0);
    for field in [SortField::Name, SortField::Random, SortField::CapturedAt] {
        query.view.sort.field = field;
        for step in 0..60 {
            library.query_window(&query, &WindowRequest::at(step * 7919 % 1_000_000, 1500))?;
        }
    }
    record("arbitrary_sorts", &library, 0);
    let mut snapshots = Vec::new();
    for generation in 0..30 {
        snapshots.push(library.projections().snapshot());
        library.rename_root(
            RootId(generation + 1),
            &format!("memory snapshot {generation}"),
            generation as i64 + 1,
        )?;
    }
    std::thread::scope(|scope| {
        let workers = (0..4)
            .map(|worker| {
                let library = &library;
                scope.spawn(move || {
                    let mut query = RootQuery {
                        scope: ItemScope::All,
                        view: Default::default(),
                    };
                    query.view.sort.field = SortField::Name;
                    for step in 0..30 {
                        library
                            .query_window(
                                &query,
                                &WindowRequest::at((worker * 30 + step) * 7919, 1500),
                            )
                            .unwrap();
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
    });
    record(
        "four_readers_and_30_retained_snapshots",
        &library,
        snapshots.len(),
    );
    drop(snapshots);
    record("released_snapshots", &library, 0);
    Ok(())
}
