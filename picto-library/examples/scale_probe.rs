use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use picto_library::predicate::{ItemSort, ViewQuerySpec};
use picto_library::query::{ItemScope, PageRequest, RootQuery};
use picto_library::selection::SelectionTarget;
use picto_library::{ImmutableMediaFacts, LabColor, Library, Lifecycle, PreparedImport, Rating};

fn main() -> picto_library::Result<()> {
    let count = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10_000);
    let directory = tempfile::tempdir().map_err(picto_library::LibraryError::Io)?;
    let library = Arc::new(Library::create(directory.path().join("scale.sqlite"))?);
    let query = RootQuery {
        scope: ItemScope::All,
        view: ViewQuerySpec {
            filter: Default::default(),
            sort: ItemSort::default(),
        },
    };

    let readers_running = Arc::new(AtomicBool::new(true));
    let concurrent_latencies = Arc::new(Mutex::new(Vec::new()));
    let reader = {
        let library = library.clone();
        let query = query.clone();
        let readers_running = readers_running.clone();
        let concurrent_latencies = concurrent_latencies.clone();
        std::thread::spawn(move || {
            while readers_running.load(Ordering::Relaxed) {
                let started = Instant::now();
                library.query(&query, &PageRequest::default()).unwrap();
                concurrent_latencies.lock().unwrap().push(started.elapsed());
                std::thread::sleep(Duration::from_millis(2));
            }
        })
    };

    let ingest_started = Instant::now();
    let mut interval_started = ingest_started;
    let mut batch_latencies = Vec::new();
    for start in (0..count).step_by(picto_library::ingest::MAX_INGEST_BATCH) {
        let end = (start + picto_library::ingest::MAX_INGEST_BATCH).min(count);
        let inputs = (start..end).map(prepared).collect::<Vec<_>>();
        let batch_started = Instant::now();
        library.ingest_batch(&inputs)?;
        batch_latencies.push(batch_started.elapsed());
        if end.is_multiple_of(100_000) || end == count {
            println!(
                "ingest_interval_end={end} ingest_interval_ms={:.3}",
                millis(interval_started.elapsed())
            );
            interval_started = Instant::now();
        }
    }
    let ingest_elapsed = ingest_started.elapsed();
    readers_running.store(false, Ordering::Relaxed);
    reader.join().unwrap();

    let read_latencies = (0..100)
        .map(|_| {
            let started = Instant::now();
            library.query(&query, &PageRequest::default()).unwrap();
            started.elapsed()
        })
        .collect::<Vec<_>>();

    let summary_started = Instant::now();
    let summary = library.selection_summary(&SelectionTarget::Query {
        query: query.clone(),
        excluded_root_ids: Vec::new(),
    })?;
    let summary_elapsed = summary_started.elapsed();

    let counts_started = Instant::now();
    let counts = library.counts()?;
    let counts_elapsed = counts_started.elapsed();

    let tag_started = Instant::now();
    library.add_tag(
        &SelectionTarget::Query {
            query: query.clone(),
            excluded_root_ids: Vec::new(),
        },
        "benchmark:selected",
    )?;
    let tag_elapsed = tag_started.elapsed();

    let lifecycle_started = Instant::now();
    library.set_lifecycle(
        &SelectionTarget::Query {
            query,
            excluded_root_ids: Vec::new(),
        },
        Lifecycle::Trash,
    )?;
    let lifecycle_elapsed = lifecycle_started.elapsed();

    println!("roots={count}");
    println!("ingest_total_ms={:.3}", millis(ingest_elapsed));
    println!(
        "ingest_database_ms_per_item={:.3}",
        millis(ingest_elapsed) / count as f64
    );
    println!(
        "ingest_batch_p95_ms={:.3}",
        millis(percentile(&mut batch_latencies, 95))
    );
    println!(
        "warm_read_p95_ms={:.3}",
        millis(percentile(&mut read_latencies.clone(), 95))
    );
    let mut concurrent_latencies = concurrent_latencies.lock().unwrap().clone();
    println!(
        "concurrent_read_p95_ms={:.3}",
        millis(percentile(&mut concurrent_latencies, 95))
    );
    println!(
        "concurrent_read_p99_ms={:.3}",
        millis(percentile(&mut concurrent_latencies, 99))
    );
    println!(
        "concurrent_read_max_ms={:.3}",
        millis(*concurrent_latencies.iter().max().unwrap())
    );
    println!("selection_summary_ms={:.3}", millis(summary_elapsed));
    println!("selection_count={}", summary.selected_count);
    println!("sidebar_counts_ms={:.3}", millis(counts_elapsed));
    println!("sidebar_active_count={}", counts.all);
    println!("add_tag_all_ms={:.3}", millis(tag_elapsed));
    println!("lifecycle_all_ms={:.3}", millis(lifecycle_elapsed));
    let gate = library.database().publication_gate_stats();
    println!("publication_gate_samples={}", gate.samples);
    println!(
        "publication_gate_p95_ms={:.3}",
        gate.p95_micros as f64 / 1_000.0
    );
    println!(
        "publication_gate_max_ms={:.3}",
        gate.max_micros as f64 / 1_000.0
    );
    let projection_bytes = library.projections().snapshot().estimated_bytes();
    println!("projection_estimated_bytes={projection_bytes}");
    println!(
        "projection_estimated_mib={:.3}",
        projection_bytes as f64 / (1024.0 * 1024.0)
    );
    let checkpoint_started = Instant::now();
    let checkpoint_bytes = library.write_projection_checkpoint()?;
    println!(
        "checkpoint_write_ms={:.3}",
        millis(checkpoint_started.elapsed())
    );
    println!("checkpoint_bytes={checkpoint_bytes}");
    drop(library);
    let reopen_started = Instant::now();
    let reopened = Library::open(directory.path().join("scale.sqlite"))?;
    let reopen_elapsed = reopen_started.elapsed();
    println!("checkpoint_reopen_ms={:.3}", millis(reopen_elapsed));
    println!(
        "checkpoint_reopen_trash_roots={}",
        reopened
            .query(
                &RootQuery {
                    scope: ItemScope::Trash,
                    view: ViewQuerySpec::default(),
                },
                &PageRequest::default(),
            )?
            .total
    );
    Ok(())
}

fn prepared(index: usize) -> PreparedImport {
    let tags = (0..20)
        .map(|offset| format!("tag:{:05}", (index * 31 + offset * 197) % 10_000))
        .collect();
    PreparedImport {
        stable_key: format!("root-{index}"),
        media_name: format!("image-{index:08}.png"),
        file_path: format!("/fixture/{index}.png"),
        facts: ImmutableMediaFacts {
            mime: "image/png".into(),
            size_bytes: 512_000 + (index % 4096) as u64,
            width: Some(1024 + (index % 1024) as u32),
            height: Some(768 + (index % 768) as u32),
            duration_ms: None,
            frame_count: Some(1),
            content_hash: format!("fixture-hash-{index}"),
            perceptual_hash: None,
            palette: vec![LabColor {
                l: (index % 100) as f32,
                a: (index % 128) as f32 - 64.0,
                b: (index * 3 % 128) as f32 - 64.0,
                weight: 1.0,
            }],
        },
        lifecycle: if index.is_multiple_of(20) {
            Lifecycle::Inbox
        } else if index.is_multiple_of(50) {
            Lifecycle::Trash
        } else {
            Lifecycle::Active
        },
        rating: Rating::Unrated,
        notes: None,
        tags,
        folders: Vec::new(),
        source_urls: vec![format!("https://fixture.invalid/{index}")],
        source_identity: Some(picto_library::SourceIdentity {
            source_key: "scale".into(),
            source_item_key: index.to_string(),
            source_text: Some(format!("fixture source {index}")),
            source_attempt_id: None,
        }),
        imported_at_ms: 1_700_000_000_000 + index as i64,
        captured_at_ms: Some(1_600_000_000_000 + index as i64),
    }
}

fn percentile(values: &mut [Duration], percentile: usize) -> Duration {
    values.sort_unstable();
    values[(values.len() - 1) * percentile / 100]
}

fn millis(value: Duration) -> f64 {
    value.as_secs_f64() * 1_000.0
}
