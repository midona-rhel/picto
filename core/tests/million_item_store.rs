//! Representative scale checks for the SQLite/WAL store and root projections.
//!
//! The normal test is deliberately smaller than the release target so it is
//! useful in CI. The ignored test runs the same path at one million roots.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use picto_core::app::{
    Application, ItemFilters, ItemId, ItemKind, ItemQuery, ItemScope, ItemSort, ItemSortField,
    ItemTarget, Lifecycle, SortDirection,
};
use picto_core::navigation_v2::CreateSmartFolderInput;
use picto_core::operations_v2::{
    DetachItemsInput, MediaMetadataPatch, OrganizeIntoCollectionInput, ReorderCollectionInput,
};
use picto_core::projection_v2::{
    RootProjectionChange, RootSummaryProjectionChange, StructureProjectionDelta,
};
use picto_core::query_v2::{
    details, query, selection_summary, selection_summary_for_application,
    sidebar_counts_for_application, ItemPageRequest,
};
use picto_core::smart_v2::SmartFolderPredicate;
use picto_core::store::{PublicationGateStats, Store};
use roaring::RoaringBitmap;
use rusqlite::OptionalExtension;

const CI_ACTIVE_ROWS: usize = 10_000;
const CI_INBOX_ROWS: usize = 2_000;
const CI_BATCH_SIZE: usize = 256;
const MILLION_ACTIVE_ROWS: usize = 1_000_000;
const MILLION_INBOX_ROWS: usize = 10_000;
const MILLION_SEED_BATCH_SIZE: usize = 16_384;
const MILLION_INGEST_BATCH_SIZE: usize = 1;
const READERS: usize = 4;
const FIXED_RATE_READERS: usize = 8;
const SYNTHETIC_TAG_COUNT: usize = 10_000;
const SYNTHETIC_TAG_END: i64 = 10_000;
const GENERAL_TAG_END: i64 = 6_000;
const CHARACTER_TAG_START: i64 = 6_001;
const CHARACTER_TAG_END: i64 = 8_500;
const SERIES_TAG_START: i64 = 8_501;
const SERIES_TAG_END: i64 = 8_750;
const CREATOR_TAG_START: i64 = 8_751;
const CREATOR_TAG_END: i64 = 9_750;
const SPECIES_TAG_START: i64 = 9_751;
const SPECIES_TAG_END: i64 = 9_950;
const RATING_TAG_START: i64 = 9_951;

#[derive(Debug, Default)]
struct IngestStats {
    elapsed: Duration,
    p50_batch: Duration,
    p95_batch: Duration,
    max_batch: Duration,
}

#[derive(Debug, Clone, Copy, Default)]
struct ExpectedCounts {
    active: i64,
    inbox: i64,
    trash: i64,
    folder_active: i64,
    bulk_folder_active: i64,
    uncategorized: i64,
}

#[derive(Debug, Default)]
struct BulkMutationStats {
    selection_summary: Duration,
    tag: Duration,
    folder: Duration,
    reader_p50: Duration,
    reader_p95: Duration,
    reader_max: Duration,
}

#[test]
fn ci_scale_store_keeps_wal_reads_queries_and_projections_bounded() {
    let result = run_scale(
        CI_ACTIVE_ROWS,
        CI_INBOX_ROWS,
        CI_BATCH_SIZE,
        Duration::from_secs(3),
        Duration::from_secs(2),
    );
    println!(
        "store_scale: rows={} tags={} memberships={} smart_memberships={} database_mib={:.2} ingest_ms={} write_p50_ms={} write_p95_ms={} max_batch_ms={} wal_read_p50_ms={} wal_read_p95_ms={} max_wal_read_ms={} gate_avg_us={} gate_max_us={} sidebar_ms={} all_ms={} inbox_ms={} trash_ms={} folder_ms={} tag_ms={} smart_tag_ms={} smart_numeric_ms={} smart_text_ms={} fts_settle_ms={} fts_query_ms={}",
        CI_ACTIVE_ROWS + CI_INBOX_ROWS,
        SYNTHETIC_TAG_COUNT,
        synthetic_membership_count(CI_ACTIVE_ROWS + CI_INBOX_ROWS),
        result.smart_memberships,
        result.database_bytes as f64 / (1024.0 * 1024.0),
        result.ingest.elapsed.as_secs_f64() * 1_000.0,
        result.ingest.p50_batch.as_secs_f64() * 1_000.0,
        result.ingest.p95_batch.as_secs_f64() * 1_000.0,
        result.ingest.max_batch.as_secs_f64() * 1_000.0,
        result.p50_wal_read.as_secs_f64() * 1_000.0,
        result.p95_wal_read.as_secs_f64() * 1_000.0,
        result.max_wal_read.as_secs_f64() * 1_000.0,
        result.gate.average_hold_micros(),
        result.gate.max_hold_micros,
        result.sidebar.as_secs_f64() * 1_000.0,
        result.all.as_secs_f64() * 1_000.0,
        result.inbox.as_secs_f64() * 1_000.0,
        result.trash.as_secs_f64() * 1_000.0,
        result.folder.as_secs_f64() * 1_000.0,
        result.tag.as_secs_f64() * 1_000.0,
        result.smart_tag.as_secs_f64() * 1_000.0,
        result.smart_numeric.as_secs_f64() * 1_000.0,
        result.smart_text.as_secs_f64() * 1_000.0,
        result.fts_settle.as_secs_f64() * 1_000.0,
        result.fts_query.as_secs_f64() * 1_000.0,
    );
    println!(
        "bulk_command_a: active_rows={} summary_ms={:.2} tag_ms={:.2} folder_ms={:.2} reader_p50_ms={:.2} reader_p95_ms={:.2} reader_max_ms={:.2}",
        CI_ACTIVE_ROWS,
        result.bulk.selection_summary.as_secs_f64() * 1_000.0,
        result.bulk.tag.as_secs_f64() * 1_000.0,
        result.bulk.folder.as_secs_f64() * 1_000.0,
        result.bulk.reader_p50.as_secs_f64() * 1_000.0,
        result.bulk.reader_p95.as_secs_f64() * 1_000.0,
        result.bulk.reader_max.as_secs_f64() * 1_000.0,
    );
}

#[test]
#[ignore = "manual million-item performance probe"]
fn million_item_store_manual_benchmark() {
    let active_rows = std::env::var("PICTO_SCALE_ACTIVE_ROWS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(MILLION_ACTIVE_ROWS)
        .clamp(1, MILLION_ACTIVE_ROWS);
    let inbox_rows = std::env::var("PICTO_SCALE_INGEST_ROWS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(MILLION_INBOX_ROWS)
        .clamp(1, MILLION_INBOX_ROWS);
    let result = run_scale(
        active_rows,
        inbox_rows,
        MILLION_INGEST_BATCH_SIZE,
        Duration::from_secs(20),
        Duration::from_secs(5),
    );
    println!(
        "million_item_store: rows={} tags={} memberships={} smart_memberships={} database_mib={:.2} concurrent_ingest_rows={} ingest_s={:.2} write_p50_ms={:.2} write_p95_ms={:.2} max_batch_s={:.2} wal_read_p50_ms={:.2} wal_read_p95_ms={:.2} max_wal_read_ms={:.2} gate_avg_us={} gate_max_us={} sidebar_ms={:.2} all_ms={:.2} inbox_ms={:.2} trash_ms={:.2} folder_ms={:.2} tag_ms={:.2} smart_tag_ms={:.2} smart_numeric_ms={:.2} smart_text_ms={:.2} fts_settle_ms={:.2} fts_query_ms={:.2}",
        active_rows + inbox_rows,
        SYNTHETIC_TAG_COUNT,
        synthetic_membership_count(active_rows + inbox_rows),
        result.smart_memberships,
        result.database_bytes as f64 / (1024.0 * 1024.0),
        inbox_rows,
        result.ingest.elapsed.as_secs_f64(),
        result.ingest.p50_batch.as_secs_f64() * 1_000.0,
        result.ingest.p95_batch.as_secs_f64() * 1_000.0,
        result.ingest.max_batch.as_secs_f64(),
        result.p50_wal_read.as_secs_f64() * 1_000.0,
        result.p95_wal_read.as_secs_f64() * 1_000.0,
        result.max_wal_read.as_secs_f64() * 1_000.0,
        result.gate.average_hold_micros(),
        result.gate.max_hold_micros,
        result.sidebar.as_secs_f64() * 1_000.0,
        result.all.as_secs_f64() * 1_000.0,
        result.inbox.as_secs_f64() * 1_000.0,
        result.trash.as_secs_f64() * 1_000.0,
        result.folder.as_secs_f64() * 1_000.0,
        result.tag.as_secs_f64() * 1_000.0,
        result.smart_tag.as_secs_f64() * 1_000.0,
        result.smart_numeric.as_secs_f64() * 1_000.0,
        result.smart_text.as_secs_f64() * 1_000.0,
        result.fts_settle.as_secs_f64() * 1_000.0,
        result.fts_query.as_secs_f64() * 1_000.0,
    );
    println!(
        "million_command_a: active_rows={} summary_s={:.3} tag_s={:.3} folder_s={:.3} reader_p50_ms={:.2} reader_p95_ms={:.2} reader_max_ms={:.2}",
        active_rows,
        result.bulk.selection_summary.as_secs_f64(),
        result.bulk.tag.as_secs_f64(),
        result.bulk.folder.as_secs_f64(),
        result.bulk.reader_p50.as_secs_f64() * 1_000.0,
        result.bulk.reader_p95.as_secs_f64() * 1_000.0,
        result.bulk.reader_max.as_secs_f64() * 1_000.0,
    );
}

struct ScaleResult {
    ingest: IngestStats,
    database_bytes: u64,
    smart_memberships: i64,
    max_wal_read: Duration,
    p50_wal_read: Duration,
    p95_wal_read: Duration,
    gate: PublicationGateStats,
    sidebar: Duration,
    all: Duration,
    inbox: Duration,
    trash: Duration,
    folder: Duration,
    tag: Duration,
    smart_tag: Duration,
    smart_numeric: Duration,
    smart_text: Duration,
    fts_settle: Duration,
    fts_query: Duration,
    bulk: BulkMutationStats,
}

fn run_scale(
    active_rows: usize,
    inbox_rows: usize,
    batch_size: usize,
    query_budget: Duration,
    read_budget: Duration,
) -> ScaleResult {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(directory.path()).unwrap());
    store
        .transaction(|transaction| {
            transaction.execute(
                "INSERT INTO folder (folder_id, folder_key, name, created_at, updated_at)
                 VALUES
                    (1, 'scale-folder', 'Scale folder', '2026-01-01', '2026-01-01'),
                    (2, 'bulk-folder', 'Bulk folder', '2026-01-01', '2026-01-01')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    insert_synthetic_tag_definitions(&store).unwrap();
    let application = Arc::new(Application::try_new(Arc::clone(&store)).unwrap());

    let journal_mode: String = store
        .read_snapshot(|connection| {
            connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))
        })
        .unwrap();
    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");

    if active_rows > 0 {
        insert_media_batches(
            &application,
            1,
            active_rows,
            MILLION_SEED_BATCH_SIZE,
            Lifecycle::Active,
        )
        .unwrap();
        // The measured tail starts from a fully settled search index, just as
        // a warm library does before a new ingest begins.
        store.refresh_search_indexes().unwrap();
        store.reset_publication_gate_stats();
    }

    // Install representative smart folders before the measured ingest so the
    // write latency includes exact incremental membership settlement.
    install_scale_smart_folders(&application);

    let total_rows = active_rows + inbox_rows;
    let (ingest, read_latency) = concurrent_ingest(
        &application,
        &store,
        active_rows + 1,
        inbox_rows,
        total_rows,
        batch_size,
        read_budget,
    );
    let expected = ExpectedCounts {
        active: active_rows as i64,
        inbox: inbox_rows as i64,
        trash: 0,
        folder_active: (active_rows / 4) as i64,
        bulk_folder_active: 0,
        uncategorized: active_rows as i64 - (active_rows / 4) as i64,
    };
    assert_projection_counts(&application, expected);
    assert!(application.projections().active_bitmap().contains(4));
    assert_sidebar_counts(&application, expected);

    let sidebar_started = Instant::now();
    let sidebar = sidebar_counts_for_application(&application).unwrap();
    let sidebar_elapsed = sidebar_started.elapsed();
    assert_sidebar_values(&sidebar, expected);
    assert_budget("sidebar counts", sidebar_elapsed, query_budget);

    let all = timed_scope_query(&store, ItemScope::All, expected.active, query_budget);
    let inbox = timed_scope_query(&store, ItemScope::Inbox, expected.inbox, query_budget);
    let trash = timed_scope_query(&store, ItemScope::Trash, expected.trash, query_budget);
    let folder = timed_scope_query(
        &store,
        ItemScope::Folder { folder_id: 1 },
        expected.folder_active,
        query_budget,
    );
    let tag = timed_tag_query(&store, active_rows, query_budget);
    let smart_tag = timed_tag_smart_folder_query(&store, active_rows, query_budget);
    let smart_numeric = timed_numeric_smart_folder_query(&store, active_rows, query_budget);
    let smart_text = timed_text_smart_folder_query(&store, active_rows, query_budget);
    let (fts_settle, fts_query) = timed_search_settlement(&store, active_rows, query_budget);
    let bulk = timed_bulk_mutations(&application, &store, active_rows, read_budget);

    // A lifecycle write must move one root in both the canonical query and
    // the in-memory bitmap without rebuilding the whole fixture.
    transition_lifecycle(&application, 4, Lifecycle::Trash);
    let after_transition = ExpectedCounts {
        active: expected.active - 1,
        trash: expected.trash + 1,
        folder_active: expected.folder_active - 1,
        bulk_folder_active: expected.active - 1,
        uncategorized: 0,
        ..expected
    };
    assert_projection_counts(&application, after_transition);
    assert!(!application.projections().active_bitmap().contains(4));
    assert!(application.projections().trash_bitmap().contains(4));
    assert_sidebar_counts(&application, after_transition);
    let transitioned_trash = timed_scope_query(
        &store,
        ItemScope::Trash,
        after_transition.trash,
        query_budget,
    );
    assert_budget(
        "post-transition trash query",
        transitioned_trash,
        query_budget,
    );

    let (database_bytes, smart_memberships) = store
        .read_snapshot(|connection| {
            let page_count: i64 =
                connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
            let page_size: i64 = connection.query_row("PRAGMA page_size", [], |row| row.get(0))?;
            let smart_memberships = connection.query_row(
                "SELECT COALESCE(SUM(member_count), 0)
                 FROM smart_folder_generation WHERE state = 'active'",
                [],
                |row| row.get(0),
            )?;
            Ok((
                page_count.saturating_mul(page_size).max(0) as u64,
                smart_memberships,
            ))
        })
        .unwrap();

    ScaleResult {
        ingest,
        database_bytes,
        smart_memberships,
        max_wal_read: read_latency.max,
        p50_wal_read: read_latency.p50,
        p95_wal_read: read_latency.p95,
        gate: store.publication_gate_stats(),
        sidebar: sidebar_elapsed,
        all,
        inbox,
        trash,
        folder,
        tag,
        smart_tag,
        smart_numeric,
        smart_text,
        fts_settle,
        fts_query,
        bulk,
    }
}

#[derive(Debug, Clone, Copy)]
struct ReadLatency {
    p50: Duration,
    p95: Duration,
    max: Duration,
}

fn concurrent_ingest(
    application: &Arc<Application>,
    store: &Arc<Store>,
    first_id: usize,
    rows: usize,
    total_rows: usize,
    batch_size: usize,
    read_budget: Duration,
) -> (IngestStats, ReadLatency) {
    let start = Arc::new(Barrier::new(READERS + 2));
    let done = Arc::new(AtomicBool::new(false));
    let max_read_micros = Arc::new(AtomicU64::new(0));
    let read_count = Arc::new(AtomicU64::new(0));
    let read_latencies = Arc::new(Mutex::new(Vec::<u64>::new()));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));

    let mut readers = Vec::with_capacity(READERS);
    for reader_id in 0..READERS {
        let application = Arc::clone(application);
        let store = Arc::clone(store);
        let start = Arc::clone(&start);
        let done = Arc::clone(&done);
        let max_read_micros = Arc::clone(&max_read_micros);
        let read_count = Arc::clone(&read_count);
        let read_latencies = Arc::clone(&read_latencies);
        let errors = Arc::clone(&errors);
        readers.push(thread::spawn(move || {
            start.wait();
            while !done.load(Ordering::Acquire) {
                let started = Instant::now();
                let observation = match reader_id {
                    0 => query(
                        &store,
                        &ItemQuery {
                            scope: ItemScope::All,
                            filters: ItemFilters::default(),
                            sort: ItemSort::default(),
                        },
                        ItemPageRequest::new(None, 50),
                    )
                    .map(|page| page.visible_item_count.unwrap_or_default()),
                    1 => sidebar_counts_for_application(&application).map(|counts| counts.all),
                    _ => store.read_snapshot(|connection| {
                        connection
                            .prepare_cached(
                                "SELECT COALESCE(MAX(item_id), 0)
                                 FROM (
                                     SELECT item_id
                                     FROM library_root
                                     WHERE lifecycle = ?1
                                     ORDER BY item_id DESC
                                     LIMIT 50
                                 )",
                            )?
                            .query_row([if reader_id == 2 { "inbox" } else { "trash" }], |row| {
                                row.get::<_, i64>(0)
                            })
                    }),
                };
                let elapsed = started.elapsed();
                record_max(&max_read_micros, elapsed);
                read_latencies
                    .lock()
                    .unwrap()
                    .push(elapsed.as_micros() as u64);
                match observation {
                    Ok(value) if (0..=total_rows as i64).contains(&value) => {
                        read_count.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(value) => {
                        errors.lock().unwrap().push(format!(
                            "reader {reader_id} observed an invalid value {value}"
                        ));
                        break;
                    }
                    Err(error) => {
                        errors
                            .lock()
                            .unwrap()
                            .push(format!("reader {reader_id} failed: {error}"));
                        break;
                    }
                }
                // Four closed-loop clients provide sustained navigation load
                // without manufacturing an unbounded queue when the host is
                // already CPU constrained by fixture construction.
                thread::sleep(Duration::from_millis(5));
            }
        }));
    }

    // Checkpointing and bounded FTS refresh run concurrently with visible
    // ingest. Neither may make the production reads miss their budget.
    let maintenance_store = Arc::clone(store);
    let maintenance_start = Arc::clone(&start);
    let maintenance_done = Arc::clone(&done);
    let maintenance_errors = Arc::clone(&errors);
    let maintenance = thread::spawn(move || {
        maintenance_start.wait();
        while !maintenance_done.load(Ordering::Acquire) {
            if let Err(error) = maintenance_store.checkpoint() {
                maintenance_errors
                    .lock()
                    .unwrap()
                    .push(format!("maintenance checkpoint failed: {error}"));
                break;
            }
            if let Err(error) = maintenance_store.maintain_search_indexes(32) {
                maintenance_errors
                    .lock()
                    .unwrap()
                    .push(format!("FTS maintenance failed: {error}"));
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
    });

    let writer_application = Arc::clone(application);
    let writer_start = Arc::clone(&start);
    let writer_done = Arc::clone(&done);
    let writer = thread::spawn(move || {
        writer_start.wait();
        let result = insert_media_batches(
            &writer_application,
            first_id,
            rows,
            batch_size,
            Lifecycle::Inbox,
        );
        writer_done.store(true, Ordering::Release);
        result
    });

    let ingest = writer.join().unwrap().unwrap();
    for reader in readers {
        reader.join().unwrap();
    }
    maintenance.join().unwrap();
    let errors = errors.lock().unwrap();
    assert!(errors.is_empty(), "concurrent WAL read errors: {errors:?}");
    assert!(
        read_count.load(Ordering::Relaxed) >= READERS as u64,
        "readers did not execute any successful snapshots"
    );

    let mut latencies = read_latencies.lock().unwrap().clone();
    latencies.sort_unstable();
    let latency = ReadLatency {
        p50: percentile(&latencies, 50),
        p95: percentile(&latencies, 95),
        max: Duration::from_micros(max_read_micros.load(Ordering::Relaxed)),
    };
    assert_budget("p95 WAL snapshot read", latency.p95, read_budget);
    (ingest, latency)
}

fn timed_bulk_mutations(
    application: &Arc<Application>,
    store: &Arc<Store>,
    active_rows: usize,
    read_budget: Duration,
) -> BulkMutationStats {
    let start = Arc::new(Barrier::new(FIXED_RATE_READERS + 1));
    let done = Arc::new(AtomicBool::new(false));
    let latencies = Arc::new(Mutex::new(Vec::<u64>::new()));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut readers = Vec::with_capacity(FIXED_RATE_READERS);

    for reader_id in 0..FIXED_RATE_READERS {
        let application = Arc::clone(application);
        let store = Arc::clone(store);
        let start = Arc::clone(&start);
        let done = Arc::clone(&done);
        let latencies = Arc::clone(&latencies);
        let errors = Arc::clone(&errors);
        readers.push(thread::spawn(move || {
            start.wait();
            while !done.load(Ordering::Acquire) {
                let started = Instant::now();
                let result = if reader_id % 2 == 0 {
                    query(
                        &store,
                        &ItemQuery {
                            scope: ItemScope::All,
                            filters: ItemFilters::default(),
                            sort: ItemSort::default(),
                        },
                        ItemPageRequest::new(None, 50),
                    )
                    .map(|page| page.visible_item_count.unwrap_or_default())
                } else {
                    sidebar_counts_for_application(&application).map(|counts| counts.all)
                };
                latencies
                    .lock()
                    .unwrap()
                    .push(started.elapsed().as_micros() as u64);
                if let Err(error) = result {
                    errors
                        .lock()
                        .unwrap()
                        .push(format!("bulk reader {reader_id} failed: {error}"));
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
        }));
    }

    start.wait();
    thread::sleep(Duration::from_millis(20));
    let target = ItemTarget::Query {
        query: ItemQuery {
            scope: ItemScope::All,
            filters: ItemFilters::default(),
            sort: ItemSort::default(),
        },
        excluded_item_ids: Vec::new(),
    };

    let summary_started = Instant::now();
    let summary = selection_summary_for_application(application, &target).unwrap();
    let selection_summary = summary_started.elapsed();
    assert_eq!(summary.selected_count, active_rows as i64);

    let tag_started = Instant::now();
    application
        .apply_tags(&target, &["benchmark:bulk".to_string()], true)
        .unwrap();
    let tag = tag_started.elapsed();

    let folder_started = Instant::now();
    application.set_folder_membership(&target, 2, true).unwrap();
    let folder = folder_started.elapsed();

    done.store(true, Ordering::Release);
    for reader in readers {
        reader.join().unwrap();
    }
    let errors = errors.lock().unwrap();
    assert!(errors.is_empty(), "concurrent bulk read errors: {errors:?}");

    let tag_id = store
        .read_snapshot(|connection| {
            connection.query_row(
                "SELECT tag_id FROM tag WHERE namespace = 'benchmark' AND subtag = 'bulk'",
                [],
                |row| row.get::<_, i64>(0),
            )
        })
        .unwrap();
    let tagged = store
        .read_snapshot(|connection| {
            connection.query_row(
                "SELECT assignment_count FROM tag_summary WHERE tag_id = ?1",
                [tag_id],
                |row| row.get::<_, i64>(0),
            )
        })
        .unwrap();
    assert_eq!(tagged, active_rows as i64);
    assert_eq!(
        application.projections().direct_tag_bitmap(tag_id).len(),
        active_rows as u64
    );
    assert_eq!(
        application.projections().direct_tag_bitmap(tag_id).len(),
        active_rows as u64
    );
    assert_eq!(
        application.projections().folder_bitmap(2).len(),
        active_rows as u64
    );

    let mut latencies = latencies.lock().unwrap().clone();
    latencies.sort_unstable();
    assert!(!latencies.is_empty(), "bulk readers did not run");
    let stats = BulkMutationStats {
        selection_summary,
        tag,
        folder,
        reader_p50: percentile(&latencies, 50),
        reader_p95: percentile(&latencies, 95),
        reader_max: Duration::from_micros(*latencies.last().unwrap()),
    };
    assert_budget("bulk mutation p95 read", stats.reader_p95, read_budget);
    stats
}

fn percentile(sorted_micros: &[u64], percentile: usize) -> Duration {
    let index = ((sorted_micros.len() - 1) * percentile).div_ceil(100);
    Duration::from_micros(sorted_micros[index])
}

fn insert_media_batches(
    application: &Application,
    first_id: usize,
    rows: usize,
    batch_size: usize,
    lifecycle: Lifecycle,
) -> Result<IngestStats, String> {
    let started = Instant::now();
    let mut max_batch = Duration::ZERO;
    let mut batch_latencies = Vec::with_capacity(rows.div_ceil(batch_size));
    for batch_start in (0..rows).step_by(batch_size) {
        let count = (rows - batch_start).min(batch_size);
        let first = first_id + batch_start;
        let batch_started = Instant::now();
        application.transaction(
            |transaction| {
                let mut insert_file = transaction.prepare(
                    "INSERT INTO media_file (
                         file_id, file_hash, mime_type, size_bytes, pixel_width,
                         pixel_height, frame_count, created_at
                     ) VALUES (?1, ?2, 'image/png', ?3, 100, 100, 1, '2026-01-01')",
                )?;
                let mut insert_item = transaction.prepare(
                    "INSERT INTO library_item (
                         item_id, item_key, kind, created_at, updated_at
                     ) VALUES (?1, ?2, 'media', '2026-01-01', '2026-01-01')",
                )?;
                let mut insert_asset = transaction.prepare(
                    "INSERT INTO media_asset (
                         item_id, file_id, name, imported_at, updated_at
                     ) VALUES (?1, ?2, ?3, '2026-01-01', '2026-01-01')",
                )?;
                let mut insert_metadata = transaction.prepare(
                    "INSERT INTO root_metadata (
                         root_item_id, name, rating, notes, source_urls_json, updated_at
                     ) VALUES (?1, ?2, NULL, NULL, '[]', '2026-01-01')",
                )?;
                let mut insert_tags = transaction.prepare(
                    "INSERT INTO root_tag(root_item_id, tag_id)
                     SELECT ?1, CAST(value AS INTEGER)
                     FROM json_each(?2)",
                )?;
                let mut insert_root = transaction
                    .prepare("INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)")?;
                let mut insert_folder_item = transaction.prepare(
                    "INSERT INTO folder_item (folder_id, item_id, position_rank)
                     VALUES (1, ?1, ?2)",
                )?;
                let mut delta = FixtureProjectionDelta {
                    structure: StructureProjectionDelta {
                        items: Vec::with_capacity(count),
                        roots: Vec::with_capacity(count),
                        folders: Vec::with_capacity(count / 4 + 1),
                        ..StructureProjectionDelta::default()
                    },
                    summaries: Vec::with_capacity(count),
                    root_tags: BTreeMap::new(),
                };

                for offset in 0..count {
                    let item_id = (first + offset) as i64;
                    let hash = format!("{item_id:064x}");
                    let key = format!("scale:{item_id}");
                    insert_file.execute(rusqlite::params![item_id, hash, 1024 + item_id])?;
                    insert_item.execute(rusqlite::params![item_id, key])?;
                    insert_asset.execute(rusqlite::params![
                        item_id,
                        item_id,
                        format!("item-{item_id}.png")
                    ])?;
                    insert_root.execute(rusqlite::params![item_id, lifecycle.as_str()])?;
                    insert_metadata
                        .execute(rusqlite::params![item_id, format!("item-{item_id}.png")])?;
                    let tag_ids = synthetic_tag_ids(item_id);
                    let encoded_tags = serde_json::to_string(&tag_ids).map_err(|error| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                    })?;
                    insert_tags.execute(rusqlite::params![item_id, encoded_tags])?;
                    for tag_id in tag_ids {
                        delta
                            .root_tags
                            .entry(tag_id)
                            .or_default()
                            .insert(item_id as u32);
                    }
                    delta
                        .structure
                        .items
                        .push(picto_core::projection_v2::ItemProjectionChange {
                            item_id,
                            kind: ItemKind::Media,
                            present: true,
                        });
                    delta.structure.media_classifications.push(
                        picto_core::projection_v2::MediaClassificationProjectionChange {
                            media_id: item_id,
                            is_image: true,
                            mime_type: "image/png".to_string(),
                        },
                    );
                    delta.structure.roots.push(RootProjectionChange {
                        item_id,
                        lifecycle: Some(lifecycle),
                    });
                    delta.summaries.push(RootSummaryProjectionChange {
                        item_id,
                        total_size_bytes: (1_024 + item_id) as u64,
                        media_count: 1,
                        rating: None,
                        display_duration_ms: None,
                        display_width: None,
                        display_height: None,
                        imported_at_ms: None,
                        modified_at_ms: None,
                    });
                    if item_id % 4 == 0 {
                        insert_folder_item.execute(rusqlite::params![item_id, item_id])?;
                        if lifecycle == Lifecycle::Active {
                            delta.structure.folders.push(
                                picto_core::projection_v2::FolderProjectionChange {
                                    folder_id: 1,
                                    item_id,
                                    present: true,
                                },
                            );
                        }
                    }
                }
                Ok(((), delta))
            },
            |projection, delta| {
                projection.apply_structure_delta(delta.structure)?;
                projection.apply_root_summary_changes(&delta.summaries, &RoaringBitmap::new())?;
                for (tag_id, roots) in delta.root_tags {
                    projection.apply_root_tag_bitmap(tag_id, &roots, true)?;
                }
                Ok(())
            },
        )?;
        let elapsed = batch_started.elapsed();
        batch_latencies.push(elapsed.as_micros() as u64);
        max_batch = max_batch.max(elapsed);
    }
    batch_latencies.sort_unstable();
    Ok(IngestStats {
        elapsed: started.elapsed(),
        p50_batch: percentile(&batch_latencies, 50),
        p95_batch: percentile(&batch_latencies, 95),
        max_batch,
    })
}

fn insert_synthetic_tag_definitions(store: &Store) -> Result<(), String> {
    store
        .transaction(|transaction| {
            let mut insert = transaction
                .prepare("INSERT INTO tag (tag_id, namespace, subtag) VALUES (?1, ?2, ?3)")?;
            for tag_id in 1..=SYNTHETIC_TAG_COUNT as i64 {
                let (namespace, ordinal) = synthetic_tag_identity(tag_id);
                insert.execute(rusqlite::params![
                    tag_id,
                    namespace,
                    format!("{namespace}_{ordinal:04}")
                ])?;
            }
            Ok(())
        })
        .map(|_| ())
}

fn synthetic_tag_identity(tag_id: i64) -> (&'static str, i64) {
    match tag_id {
        1..=GENERAL_TAG_END => ("general", tag_id - 1),
        CHARACTER_TAG_START..=CHARACTER_TAG_END => ("character", tag_id - CHARACTER_TAG_START),
        SERIES_TAG_START..=SERIES_TAG_END => ("series", tag_id - SERIES_TAG_START),
        CREATOR_TAG_START..=CREATOR_TAG_END => ("creator", tag_id - CREATOR_TAG_START),
        SPECIES_TAG_START..=SPECIES_TAG_END => ("species", tag_id - SPECIES_TAG_START),
        RATING_TAG_START..=SYNTHETIC_TAG_END => ("rating", tag_id - RATING_TAG_START),
        _ => unreachable!("synthetic tag id outside fixture"),
    }
}

fn synthetic_tag_ids(item_id: i64) -> Vec<i64> {
    let desired = synthetic_tags_per_item(item_id);
    let mut tags = Vec::with_capacity(desired);
    tags.push(CREATOR_TAG_START + item_id.rem_euclid(CREATOR_TAG_END - CREATOR_TAG_START + 1));
    if desired == 1 {
        return tags;
    }

    tags.push(SERIES_TAG_START + item_id.rem_euclid(SERIES_TAG_END - SERIES_TAG_START + 1));
    tags.push(SPECIES_TAG_START + item_id.rem_euclid(SPECIES_TAG_END - SPECIES_TAG_START + 1));
    tags.push(RATING_TAG_START + item_id.rem_euclid(SYNTHETIC_TAG_END - RATING_TAG_START + 1));

    let character_count = match desired {
        20 => 2,
        32 => 4,
        64 => 8,
        128 => 16,
        _ => 24,
    };
    let character_span = CHARACTER_TAG_END - CHARACTER_TAG_START + 1;
    for ordinal in 0..character_count {
        tags.push(
            CHARACTER_TAG_START + (item_id * 29 + ordinal as i64 * 31).rem_euclid(character_span),
        );
    }
    let general_count = desired - tags.len();
    for ordinal in 0..general_count {
        tags.push(1 + (item_id * 131 + ordinal as i64 * 17).rem_euclid(GENERAL_TAG_END));
    }
    tags
}

fn synthetic_tags_per_item(item_id: i64) -> usize {
    match item_id.rem_euclid(100) {
        0..=69 => 20,
        70..=89 => 32,
        90..=96 => 64,
        97..=98 => 128,
        _ => 250,
    }
}

fn synthetic_membership_count(rows: usize) -> usize {
    (1..=rows as i64).map(synthetic_tags_per_item).sum()
}

fn timed_tag_query(store: &Store, rows: usize, budget: Duration) -> Duration {
    let tag = "creator:creator_0000";
    let expected = expected_creator_matches(rows);
    let started = Instant::now();
    let page = query(
        store,
        &ItemQuery {
            scope: ItemScope::All,
            filters: ItemFilters {
                include_tags: vec![tag.to_string()],
                ..ItemFilters::default()
            },
            sort: ItemSort::default(),
        },
        ItemPageRequest::new(None, 50),
    )
    .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(page.visible_item_count, Some(expected));
    assert_budget("tag query", elapsed, budget);
    elapsed
}

fn timed_tag_smart_folder_query(store: &Store, rows: usize, budget: Duration) -> Duration {
    insert_smart_folder(store, 1, tag_smart_folder_predicate());
    let started = Instant::now();
    let page = query(
        store,
        &ItemQuery {
            scope: ItemScope::SmartFolder { smart_folder_id: 1 },
            filters: ItemFilters::default(),
            sort: ItemSort::default(),
        },
        ItemPageRequest::new(None, 50),
    )
    .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(
        page.visible_item_count,
        Some(expected_creator_matches(rows))
    );
    assert_budget("smart-folder query", elapsed, budget);
    elapsed
}

fn tag_smart_folder_predicate() -> serde_json::Value {
    serde_json::json!({
        "groups": [{
            "match_mode": "all",
            "negate": false,
            "rules": [{
                "field": "tags",
                "op": "include",
                "values": ["creator:creator_0000"]
            }]
        }]
    })
}

fn timed_numeric_smart_folder_query(store: &Store, rows: usize, budget: Duration) -> Duration {
    let minimum_size = 501_024_i64;
    insert_smart_folder(store, 2, numeric_smart_folder_predicate(minimum_size));
    let expected = (1..=rows as i64)
        .filter(|item_id| 1_024 + item_id >= minimum_size)
        .count() as i64;
    timed_smart_scope(store, 2, expected, "numeric smart-folder query", budget)
}

fn numeric_smart_folder_predicate(minimum_size: i64) -> serde_json::Value {
    serde_json::json!({
        "groups": [{
            "match_mode": "all",
            "negate": false,
            "rules": [{
                "field": "file_size",
                "op": "gte",
                "value": minimum_size
            }]
        }]
    })
}

fn timed_text_smart_folder_query(store: &Store, rows: usize, budget: Duration) -> Duration {
    let needle = "9999";
    insert_smart_folder(store, 3, text_smart_folder_predicate(needle));
    let expected = (1..=rows as i64)
        .filter(|item_id| format!("item-{item_id}.png").contains(needle))
        .count() as i64;
    timed_smart_scope(store, 3, expected, "text smart-folder query", budget)
}

fn text_smart_folder_predicate(needle: &str) -> serde_json::Value {
    serde_json::json!({
        "groups": [{
            "match_mode": "all",
            "negate": false,
            "rules": [{
                "field": "name",
                "op": "contains",
                "value": needle
            }]
        }]
    })
}

fn install_scale_smart_folders(application: &Application) {
    for (name, predicate) in [
        ("Scale creator", tag_smart_folder_predicate()),
        ("Scale size", numeric_smart_folder_predicate(501_024)),
        ("Scale text", text_smart_folder_predicate("9999")),
        ("Scale rating", rating_smart_folder_predicate(4)),
    ] {
        let predicate = serde_json::from_value(predicate).unwrap();
        application
            .create_smart_folder_v2(&CreateSmartFolderInput {
                name: name.to_string(),
                parent_id: None,
                predicate,
                icon: None,
                color: None,
                notes: None,
                sort_field: None,
                sort_order: None,
            })
            .unwrap();
    }
}

fn rating_smart_folder_predicate(minimum_rating: i64) -> serde_json::Value {
    serde_json::json!({
        "groups": [{
            "match_mode": "all",
            "negate": false,
            "rules": [{
                "field": "rating",
                "op": "gte",
                "value": minimum_rating
            }]
        }]
    })
}

fn insert_smart_folder(store: &Store, smart_folder_id: i64, predicate: serde_json::Value) {
    store
        .transaction(|transaction| {
            transaction.execute(
                "INSERT OR IGNORE INTO smart_folder (
                     smart_folder_id, smart_folder_key, name, predicate_json,
                     created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, 'now', 'now')",
                rusqlite::params![
                    smart_folder_id,
                    format!("scale-smart:{smart_folder_id}"),
                    format!("Scale smart folder {smart_folder_id}"),
                    predicate.to_string()
                ],
            )?;
            Ok(())
        })
        .unwrap();
}

fn timed_smart_scope(
    store: &Store,
    smart_folder_id: i64,
    expected: i64,
    label: &str,
    budget: Duration,
) -> Duration {
    let started = Instant::now();
    let page = query(
        store,
        &ItemQuery {
            scope: ItemScope::SmartFolder { smart_folder_id },
            filters: ItemFilters::default(),
            sort: ItemSort::default(),
        },
        ItemPageRequest::new(None, 50),
    )
    .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(page.visible_item_count, Some(expected));
    assert_budget(label, elapsed, budget);
    elapsed
}

fn timed_search_settlement(store: &Store, rows: usize, budget: Duration) -> (Duration, Duration) {
    let settle_started = Instant::now();
    store.refresh_search_indexes().unwrap();
    let settle_elapsed = settle_started.elapsed();
    assert_budget("FTS settlement", settle_elapsed, budget);

    let item_id = rows as i64;
    let query_started = Instant::now();
    let page = query(
        store,
        &ItemQuery {
            scope: ItemScope::All,
            filters: ItemFilters {
                text: Some(format!("item-{item_id}.png")),
                ..ItemFilters::default()
            },
            sort: ItemSort::default(),
        },
        ItemPageRequest::new(None, 50),
    )
    .unwrap();
    let query_elapsed = query_started.elapsed();
    assert_eq!(page.visible_item_count, Some(1));
    assert_eq!(page.items[0].item_id.0, item_id);
    assert_budget("settled FTS query", query_elapsed, budget);
    (settle_elapsed, query_elapsed)
}

fn expected_creator_matches(rows: usize) -> i64 {
    (1..=rows as i64)
        .filter(|item_id| synthetic_tag_ids(*item_id).contains(&CREATOR_TAG_START))
        .count() as i64
}

fn transition_lifecycle(application: &Application, item_id: i64, lifecycle: Lifecycle) {
    application
        .transaction(
            |transaction| {
                transaction.execute(
                    "UPDATE library_root SET lifecycle = ?1 WHERE item_id = ?2",
                    rusqlite::params![lifecycle.as_str(), item_id],
                )?;
                Ok((
                    (),
                    StructureProjectionDelta {
                        roots: vec![RootProjectionChange {
                            item_id,
                            lifecycle: Some(lifecycle),
                        }],
                        ..StructureProjectionDelta::default()
                    },
                ))
            },
            |projection, delta| projection.apply_structure_delta(delta),
        )
        .unwrap();
}

fn timed_scope_query(store: &Store, scope: ItemScope, expected: i64, budget: Duration) -> Duration {
    let started = Instant::now();
    let page = query(
        store,
        &ItemQuery {
            scope,
            filters: ItemFilters::default(),
            sort: ItemSort::default(),
        },
        ItemPageRequest::new(None, 50),
    )
    .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(page.visible_item_count, Some(expected));
    assert_eq!(page.visible_media_count, Some(expected));
    assert!(page.items.len() <= 50);
    assert_budget("scope query", elapsed, budget);
    elapsed
}

fn assert_projection_counts(application: &Application, expected: ExpectedCounts) {
    assert_eq!(
        application.projections().active_bitmap().len(),
        expected.active as u64
    );
    assert_eq!(
        application.projections().inbox_bitmap().len(),
        expected.inbox as u64
    );
    assert_eq!(
        application.projections().trash_bitmap().len(),
        expected.trash as u64
    );
    assert_eq!(
        application.projections().folder_bitmap(1).len(),
        expected.folder_active as u64
    );
    assert_eq!(
        application.projections().folder_bitmap(2).len(),
        expected.bulk_folder_active as u64
    );
}

fn assert_sidebar_counts(application: &Application, expected: ExpectedCounts) {
    let counts = sidebar_counts_for_application(application).unwrap();
    assert_sidebar_values(&counts, expected);
}

fn assert_sidebar_values(counts: &picto_core::query_v2::SidebarCounts, expected: ExpectedCounts) {
    assert_eq!(counts.all, expected.active);
    assert_eq!(counts.inbox, expected.inbox);
    assert_eq!(counts.trash, expected.trash);
    assert_eq!(counts.recently_viewed, 0);
    assert_eq!(counts.untagged, 0);
    assert_eq!(counts.uncategorized, expected.uncategorized);
    assert_eq!(counts.duplicates, 0);
    assert_eq!(
        counts.folders,
        vec![
            picto_core::query_v2::ScopeCount {
                id: 1,
                count: expected.folder_active,
            },
            picto_core::query_v2::ScopeCount {
                id: 2,
                count: expected.bulk_folder_active,
            },
        ]
    );
}

fn record_max(max_micros: &AtomicU64, elapsed: Duration) {
    let candidate = elapsed.as_micros() as u64;
    let mut current = max_micros.load(Ordering::Relaxed);
    while candidate > current {
        match max_micros.compare_exchange_weak(
            current,
            candidate,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(previous) => current = previous,
        }
    }
}

fn assert_budget(label: &str, elapsed: Duration, budget: Duration) {
    assert!(
        elapsed <= budget,
        "{label} exceeded budget: {:.2}s > {:.2}s",
        elapsed.as_secs_f64(),
        budget.as_secs_f64()
    );
}

// Broad public-operation coverage. Fixture construction remains direct and
// deterministic so timings measure mutation and settlement rather than test
// data generation.
const FIXED_READER_PERIOD: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, Default)]
struct FixedRateReadStats {
    samples: usize,
    missed_deadlines: usize,
    p50: Duration,
    p95: Duration,
    p99: Duration,
    max: Duration,
}

#[derive(Debug, Clone, Copy, Default)]
struct MutationMeasurement {
    elapsed: Duration,
    revision_delta: u64,
    wal_growth_bytes: i64,
    history_entry_delta: i64,
    gate: PublicationGateStats,
    readers: FixedRateReadStats,
}

#[derive(Default)]
struct FixtureProjectionDelta {
    structure: StructureProjectionDelta,
    summaries: Vec<RootSummaryProjectionChange>,
    root_tags: BTreeMap<i64, RoaringBitmap>,
}

#[test]
fn broad_mutation_matrix_1_50_and_1000_is_exact() {
    run_broad_mutation_matrix(1_100, &[1, 50, 1_000], false);
}

#[test]
#[ignore = "manual 100k mutation and fixed-rate-reader benchmark"]
fn broad_mutation_matrix_100k_is_exact_under_fixed_rate_readers() {
    let rows = std::env::var("PICTO_MUTATION_MATRIX_ROWS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100_000)
        .clamp(1_000, 100_000);
    run_broad_mutation_matrix(rows, &[rows], true);
}

#[test]
#[ignore = "manual 100k query-selection mutation benchmark"]
fn query_selection_mutations_100k_are_exact_under_fixed_rate_readers() {
    let rows = benchmark_rows("PICTO_QUERY_MUTATION_ROWS", 100_000, 1_000, 100_000);
    let (directory, store, application) = prepare_mutation_fixture(rows);
    let target = query_target_for_tail(rows, rows);

    exercise_selection_summary(
        &application,
        &store,
        directory.path(),
        &target,
        rows,
        exact_size_sum(1, rows),
    );
    exercise_tag_membership(&application, &store, directory.path(), &target, rows);
    exercise_folder_membership(&application, &store, directory.path(), &target, rows);
    exercise_lifecycle(&application, &store, directory.path(), &target, rows, rows);
}

#[test]
#[ignore = "manual 100k explicit/query/folder/tag/smart/range selection matrix"]
fn selection_source_matrix_100k_is_exact_under_fixed_rate_readers() {
    let rows = benchmark_rows("PICTO_SELECTION_MATRIX_ROWS", 100_000, 10_000, 100_000);
    let selected_sources = std::env::var("PICTO_SELECTION_SOURCES")
        .ok()
        .map(|sources| {
            sources
                .split(',')
                .map(str::trim)
                .filter(|source| !source.is_empty())
                .map(str::to_string)
                .collect::<std::collections::BTreeSet<_>>()
        });
    let summary_only =
        std::env::var("PICTO_SELECTION_OPERATIONS").is_ok_and(|operations| operations == "summary");
    let (directory, store, application) = prepare_mutation_fixture(rows);
    let range_count = rows.min(1_000);
    let range_first = rows / 3;
    let explicit_count = rows.min(1_000);
    let explicit_first = rows - explicit_count + 1;
    let folder_count = rows / 4;
    let folder_size = exact_arithmetic_size_sum(4, 4, folder_count);
    let tagged_ids = (1..=rows)
        .filter(|item_id| synthetic_tag_ids(*item_id as i64).contains(&CREATOR_TAG_START))
        .collect::<Vec<_>>();
    let tagged_size = tagged_ids
        .iter()
        .map(|item_id| 1_024 + *item_id as i64)
        .sum::<i64>();

    let cases = vec![
        (
            "explicit",
            explicit_target(explicit_first, explicit_count),
            explicit_count,
            exact_size_sum(explicit_first, explicit_count),
        ),
        ("query", all_target(), rows, exact_size_sum(1, rows)),
        (
            "folder",
            ItemTarget::Query {
                query: ItemQuery {
                    scope: ItemScope::Folder { folder_id: 1 },
                    filters: ItemFilters::default(),
                    sort: ItemSort::default(),
                },
                excluded_item_ids: Vec::new(),
            },
            folder_count,
            folder_size,
        ),
        (
            "tag",
            ItemTarget::Query {
                query: ItemQuery {
                    scope: ItemScope::All,
                    filters: ItemFilters {
                        include_tags: vec!["creator:creator_0000".to_string()],
                        ..ItemFilters::default()
                    },
                    sort: ItemSort::default(),
                },
                excluded_item_ids: Vec::new(),
            },
            tagged_ids.len(),
            tagged_size,
        ),
        (
            "range",
            range_target(range_first, range_count),
            range_count,
            exact_size_sum(range_first, range_count),
        ),
        (
            "smart",
            ItemTarget::Query {
                query: ItemQuery {
                    scope: ItemScope::SmartFolder { smart_folder_id: 1 },
                    filters: ItemFilters::default(),
                    sort: ItemSort::default(),
                },
                excluded_item_ids: Vec::new(),
            },
            tagged_ids.len(),
            tagged_size,
        ),
    ];

    for (source, target, cardinality, expected_size) in cases {
        if selected_sources
            .as_ref()
            .is_some_and(|selected| !selected.contains(source))
        {
            continue;
        }
        println!("selection_source source={source} cardinality={cardinality}");
        exercise_selection_summary(
            &application,
            &store,
            directory.path(),
            &target,
            cardinality,
            expected_size,
        );
        if summary_only {
            continue;
        }
        exercise_tag_membership(&application, &store, directory.path(), &target, cardinality);
        exercise_folder_membership(&application, &store, directory.path(), &target, cardinality);
        exercise_rating(&application, &store, directory.path(), &target, cardinality);
        exercise_lifecycle(
            &application,
            &store,
            directory.path(),
            &target,
            cardinality,
            rows,
        );
    }
}

#[test]
#[ignore = "manual representative 100k topology fixture"]
fn representative_100k_fixture_has_exact_lifecycle_organization_and_groups() {
    let rows = benchmark_rows("PICTO_REPRESENTATIVE_ROWS", 100_000, 20_000, 100_000);
    let active_rows = rows * 80 / 100;
    let inbox_rows = rows * 15 / 100;
    let trash_rows = rows - active_rows - inbox_rows;
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(directory.path()).unwrap());
    store
        .transaction(|transaction| {
            transaction.execute(
                "INSERT INTO folder (folder_id, folder_key, name, created_at, updated_at)
                 VALUES
                    (1, 'representative-seed', 'Representative seed', '2026-01-01', '2026-01-01'),
                    (2, 'representative-target', 'Representative target', '2026-01-01', '2026-01-01')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    insert_synthetic_tag_definitions(&store).unwrap();
    let application = Arc::new(Application::try_new(Arc::clone(&store)).unwrap());

    let active = insert_media_batches(
        &application,
        1,
        active_rows,
        MILLION_SEED_BATCH_SIZE,
        Lifecycle::Active,
    )
    .unwrap();
    let inbox = insert_media_batches(
        &application,
        active_rows + 1,
        inbox_rows,
        MILLION_SEED_BATCH_SIZE,
        Lifecycle::Inbox,
    )
    .unwrap();
    let trash = insert_media_batches(
        &application,
        active_rows + inbox_rows + 1,
        trash_rows,
        MILLION_SEED_BATCH_SIZE,
        Lifecycle::Trash,
    )
    .unwrap();
    store.refresh_search_indexes().unwrap();
    install_scale_smart_folders(&application);
    assert_root_state(
        &application,
        active_rows as i64,
        inbox_rows as i64,
        trash_rows as i64,
    );

    let hidden_organization: (i64, i64, i64, i64) = store
        .read_snapshot(|connection| {
            Ok((
                connection.query_row(
                    "SELECT COUNT(*) FROM root_tag rt JOIN library_root lr
                     ON lr.item_id = rt.root_item_id WHERE lr.lifecycle = 'inbox'",
                    [],
                    |row| row.get(0),
                )?,
                connection.query_row(
                    "SELECT COUNT(*) FROM root_tag rt JOIN library_root lr
                     ON lr.item_id = rt.root_item_id WHERE lr.lifecycle = 'trash'",
                    [],
                    |row| row.get(0),
                )?,
                connection.query_row(
                    "SELECT COUNT(*) FROM folder_item fi JOIN library_root lr
                     ON lr.item_id = fi.item_id WHERE lr.lifecycle = 'inbox'",
                    [],
                    |row| row.get(0),
                )?,
                connection.query_row(
                    "SELECT COUNT(*) FROM folder_item fi JOIN library_root lr
                     ON lr.item_id = fi.item_id WHERE lr.lifecycle = 'trash'",
                    [],
                    |row| row.get(0),
                )?,
            ))
        })
        .unwrap();
    assert!(hidden_organization.0 > 0 && hidden_organization.1 > 0);
    assert_eq!(
        hidden_organization.2,
        multiples_in_range(active_rows + 1, inbox_rows, 4) as i64
    );
    assert_eq!(
        hidden_organization.3,
        multiples_in_range(active_rows + inbox_rows + 1, trash_rows, 4) as i64
    );
    assert_eq!(
        application.projections().folder_bitmap(1).len(),
        (active_rows / 4) as u64
    );
    let (active_assignments, visible_assignments, visible_folder_roots): (i64, i64, i64) = store
        .read_snapshot(|connection| {
            Ok((
                connection.query_row(
                    "SELECT COUNT(*) FROM root_tag rt JOIN library_root lr
                     ON lr.item_id = rt.root_item_id WHERE lr.lifecycle = 'active'",
                    [],
                    |row| row.get(0),
                )?,
                connection.query_row(
                    "SELECT COALESCE(SUM(visible_root_count), 0) FROM tag_summary",
                    [],
                    |row| row.get(0),
                )?,
                connection.query_row(
                    "SELECT visible_root_count FROM folder_summary WHERE folder_id = 1",
                    [],
                    |row| row.get(0),
                )?,
            ))
        })
        .unwrap();
    assert_eq!(visible_assignments, active_assignments);
    assert_eq!(visible_folder_roots, (active_rows / 4) as i64);

    let mut next_member = 1_usize;
    let mut grouped_members = 0_usize;
    let mut groups = 0_usize;
    for group_size in [2_usize, 50, 1_000, 10_000] {
        if next_member + group_size > active_rows {
            break;
        }
        let started = Instant::now();
        let result = application
            .organize_into_collection(OrganizeIntoCollectionInput {
                target: explicit_target(next_member, group_size),
                label: Some(format!("Fixture group {group_size}")),
                winning_collection_id: None,
            })
            .unwrap();
        assert_collection_state(&application, result.collection_id, group_size as i64);
        println!(
            "fixture_stage stage=group_create cardinality={} elapsed_ms={:.3}",
            group_size,
            started.elapsed().as_secs_f64() * 1_000.0
        );
        next_member += group_size;
        grouped_members += group_size;
        groups += 1;
    }
    assert_root_state(
        &application,
        (active_rows - grouped_members + groups) as i64,
        inbox_rows as i64,
        trash_rows as i64,
    );
    println!(
        "fixture_stage rows={} tags={} memberships={} active_ms={:.3} inbox_ms={:.3} trash_ms={:.3} hidden_inbox_tags={} hidden_trash_tags={}",
        rows,
        SYNTHETIC_TAG_COUNT,
        synthetic_membership_count(rows),
        active.elapsed.as_secs_f64() * 1_000.0,
        inbox.elapsed.as_secs_f64() * 1_000.0,
        trash.elapsed.as_secs_f64() * 1_000.0,
        hidden_organization.0,
        hidden_organization.1,
    );
}

#[test]
#[ignore = "manual 100k rating mutation benchmark"]
fn rating_mutation_100k_is_exact_under_fixed_rate_readers() {
    let rows = benchmark_rows("PICTO_RATING_MATRIX_ROWS", 100_000, 1_000, 100_000);
    let (directory, store, application) = prepare_mutation_fixture(rows);
    let target = query_target_for_tail(rows, rows);
    exercise_rating(&application, &store, directory.path(), &target, rows);
}

#[test]
#[ignore = "manual million-root mutation and concurrent-reader benchmark"]
fn broad_mutation_matrix_million_manual() {
    let rows = std::env::var("PICTO_MUTATION_MATRIX_ROWS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1_000_000)
        .clamp(100_000, 1_000_000);
    run_broad_mutation_matrix(rows, &[rows], true);
}

#[test]
#[ignore = "manual 100k smart-folder revalidation benchmark"]
fn smart_folder_revalidation_100k_is_exact_under_fixed_rate_readers() {
    let rows = benchmark_rows("PICTO_STRUCTURAL_MATRIX_ROWS", 100_000, 1_000, 100_000);
    let (directory, store, application) = prepare_mutation_fixture(rows);

    exercise_smart_folder_mutations(&application, &store, directory.path(), &[rows], rows);
}

#[test]
#[ignore = "manual 100k group-operation benchmark"]
fn group_operations_100k_is_exact_under_fixed_rate_readers() {
    let rows = benchmark_rows("PICTO_GROUP_MATRIX_ROWS", 100_000, 1_000, 100_000);
    let (directory, store, application) = prepare_mutation_fixture(rows);

    exercise_group_operations(&application, &store, directory.path(), rows, rows);
}

#[test]
#[ignore = "manual 100k permanent-delete benchmark"]
fn permanent_delete_100k_is_exact_under_fixed_rate_readers() {
    let rows = benchmark_rows("PICTO_DELETE_MATRIX_ROWS", 100_000, 1_000, 100_000);
    let (directory, store, application) = prepare_mutation_fixture(rows);

    exercise_delete(&application, &store, directory.path(), all_target(), rows);
    assert_root_state(&application, 0, 0, 0);
    for table in [
        "library_item",
        "library_root",
        "media_asset",
        "media_file",
        "folder_item",
        "root_tag",
        "root_summary",
    ] {
        assert_table_count(&store, table, 0);
    }
}

fn benchmark_rows(variable: &str, default: usize, minimum: usize, maximum: usize) -> usize {
    std::env::var(variable)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
        .clamp(minimum, maximum)
}

fn prepare_mutation_fixture(rows: usize) -> (tempfile::TempDir, Arc<Store>, Arc<Application>) {
    let directory = tempfile::tempdir().unwrap();
    let open_started = Instant::now();
    let store = Arc::new(Store::open(directory.path()).unwrap());
    let open_elapsed = open_started.elapsed();
    store
        .transaction(|transaction| {
            transaction.execute(
                "INSERT INTO folder (folder_id, folder_key, name, created_at, updated_at)
                 VALUES
                    (1, 'matrix-seed', 'Matrix seed', '2026-01-01', '2026-01-01'),
                    (2, 'matrix-target', 'Matrix target', '2026-01-01', '2026-01-01')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let tags_started = Instant::now();
    insert_synthetic_tag_definitions(&store).unwrap();
    let tags_elapsed = tags_started.elapsed();
    let application_started = Instant::now();
    let application = Arc::new(Application::try_new(Arc::clone(&store)).unwrap());
    let application_elapsed = application_started.elapsed();
    let seed = insert_media_batches(
        &application,
        1,
        rows,
        MILLION_SEED_BATCH_SIZE,
        Lifecycle::Active,
    )
    .unwrap();
    let fts_started = Instant::now();
    store.refresh_search_indexes().unwrap();
    let fts_elapsed = fts_started.elapsed();
    let smart_started = Instant::now();
    install_scale_smart_folders(&application);
    let smart_elapsed = smart_started.elapsed();
    assert_root_state(&application, rows as i64, 0, 0);
    println!(
        "fixture_stage rows={} tags={} memberships={} open_ms={:.3} tag_seed_ms={:.3} application_init_ms={:.3} media_seed_ms={:.3} media_batch_p95_ms={:.3} fts_settle_ms={:.3} smart_generation_ms={:.3}",
        rows,
        SYNTHETIC_TAG_COUNT,
        synthetic_membership_count(rows),
        open_elapsed.as_secs_f64() * 1_000.0,
        tags_elapsed.as_secs_f64() * 1_000.0,
        application_elapsed.as_secs_f64() * 1_000.0,
        seed.elapsed.as_secs_f64() * 1_000.0,
        seed.p95_batch.as_secs_f64() * 1_000.0,
        fts_elapsed.as_secs_f64() * 1_000.0,
        smart_elapsed.as_secs_f64() * 1_000.0,
    );
    (directory, store, application)
}

fn run_broad_mutation_matrix(rows: usize, cardinalities: &[usize], delete_all: bool) {
    let (directory, store, application) = prepare_mutation_fixture(rows);

    for &cardinality in cardinalities {
        assert!(cardinality <= rows);
        let target = query_target_for_tail(rows, cardinality);
        let first_selected = rows - cardinality + 1;
        exercise_selection_summary(
            &application,
            &store,
            directory.path(),
            &target,
            cardinality,
            exact_size_sum(first_selected, cardinality),
        );

        exercise_tag_membership(&application, &store, directory.path(), &target, cardinality);
        exercise_folder_membership(&application, &store, directory.path(), &target, cardinality);
        exercise_lifecycle(
            &application,
            &store,
            directory.path(),
            &target,
            cardinality,
            rows,
        );
    }

    exercise_smart_folder_mutations(&application, &store, directory.path(), cardinalities, rows);
    for &cardinality in cardinalities {
        exercise_group_operations(
            &application,
            &store,
            directory.path(),
            cardinality.max(3).min(rows),
            rows,
        );
    }

    // Keep rating last: a failure must not hide coverage for the independent structural paths.
    for &cardinality in cardinalities {
        let target = query_target_for_tail(rows, cardinality);
        exercise_rating(&application, &store, directory.path(), &target, cardinality);
    }

    if delete_all {
        exercise_delete(&application, &store, directory.path(), all_target(), rows);
        assert_root_state(&application, 0, 0, 0);
        assert_table_count(&store, "library_item", 0);
        assert_table_count(&store, "media_asset", 0);
        assert_table_count(&store, "media_file", 0);
        assert_table_count(&store, "folder_item", 0);
    } else {
        let mut first = 1;
        for count in [1_usize, 50, 1_000] {
            if first + count - 1 > rows {
                break;
            }
            exercise_delete(
                &application,
                &store,
                directory.path(),
                explicit_target(first, count),
                count,
            );
            first += count;
        }
        assert_root_state(&application, (rows - first + 1) as i64, 0, 0);
    }
}

fn exercise_selection_summary(
    application: &Arc<Application>,
    store: &Arc<Store>,
    library_root: &std::path::Path,
    target: &ItemTarget,
    cardinality: usize,
    expected_size_bytes: i64,
) {
    let (summary, measurement) = measure_mutation(
        "selection.summary",
        cardinality,
        application,
        store,
        library_root,
        || selection_summary_for_application(application, target),
    );
    let selection = summary.unwrap();
    assert_eq!(selection.selected_count, cardinality as i64);
    assert_eq!(selection.stats.total_size_bytes, Some(expected_size_bytes));
    assert!(selection.stats.all_media_are_images);
    print_mutation_metric("selection.summary", cardinality, measurement);
}

fn exercise_tag_membership(
    application: &Arc<Application>,
    store: &Arc<Store>,
    library_root: &std::path::Path,
    target: &ItemTarget,
    cardinality: usize,
) {
    let tag = format!("benchmark:matrix_{cardinality}");
    let (add_result, add) = measure_mutation(
        "tags.add",
        cardinality,
        application,
        store,
        library_root,
        || application.apply_tags(target, std::slice::from_ref(&tag), true),
    );
    add_result.unwrap();
    measure_validation("tags.add", cardinality, || {
        assert_tag_cardinality(application, store, &tag, cardinality as i64)
    });
    let (undo_result, undo) = measure_mutation(
        "tags.add.undo",
        cardinality,
        application,
        store,
        library_root,
        || application.undo(),
    );
    undo_result.unwrap();
    measure_validation("tags.add.undo", cardinality, || {
        assert_tag_cardinality(application, store, &tag, 0)
    });
    let (redo_result, redo) = measure_mutation(
        "tags.add.redo",
        cardinality,
        application,
        store,
        library_root,
        || application.redo(),
    );
    redo_result.unwrap();
    measure_validation("tags.add.redo", cardinality, || {
        assert_tag_cardinality(application, store, &tag, cardinality as i64)
    });
    let (remove_result, remove) = measure_mutation(
        "tags.remove",
        cardinality,
        application,
        store,
        library_root,
        || application.apply_tags(target, std::slice::from_ref(&tag), false),
    );
    remove_result.unwrap();
    measure_validation("tags.remove", cardinality, || {
        assert_tag_cardinality(application, store, &tag, 0)
    });
    for (label, value) in [
        ("tags.add", add),
        ("tags.add.undo", undo),
        ("tags.add.redo", redo),
        ("tags.remove", remove),
    ] {
        print_mutation_metric(label, cardinality, value);
    }
}

fn exercise_folder_membership(
    application: &Arc<Application>,
    store: &Arc<Store>,
    library_root: &std::path::Path,
    target: &ItemTarget,
    cardinality: usize,
) {
    let (add_result, add) = measure_mutation(
        "folders.add",
        cardinality,
        application,
        store,
        library_root,
        || application.set_folder_membership(target, 2, true),
    );
    add_result.unwrap();
    measure_validation("folders.add", cardinality, || {
        assert_folder_cardinality(application, store, 2, cardinality as i64)
    });
    let (undo_result, undo) = measure_mutation(
        "folders.add.undo",
        cardinality,
        application,
        store,
        library_root,
        || application.undo(),
    );
    undo_result.unwrap();
    measure_validation("folders.add.undo", cardinality, || {
        assert_folder_cardinality(application, store, 2, 0)
    });
    let (redo_result, redo) = measure_mutation(
        "folders.add.redo",
        cardinality,
        application,
        store,
        library_root,
        || application.redo(),
    );
    redo_result.unwrap();
    measure_validation("folders.add.redo", cardinality, || {
        assert_folder_cardinality(application, store, 2, cardinality as i64)
    });
    let (remove_result, remove) = measure_mutation(
        "folders.remove",
        cardinality,
        application,
        store,
        library_root,
        || application.set_folder_membership(target, 2, false),
    );
    remove_result.unwrap();
    measure_validation("folders.remove", cardinality, || {
        assert_folder_cardinality(application, store, 2, 0)
    });
    for (label, value) in [
        ("folders.add", add),
        ("folders.add.undo", undo),
        ("folders.add.redo", redo),
        ("folders.remove", remove),
    ] {
        print_mutation_metric(label, cardinality, value);
    }
}

fn exercise_lifecycle(
    application: &Arc<Application>,
    store: &Arc<Store>,
    library_root: &std::path::Path,
    target: &ItemTarget,
    cardinality: usize,
    total_rows: usize,
) {
    let (trash_result, trash) = measure_mutation(
        "lifecycle.trash",
        cardinality,
        application,
        store,
        library_root,
        || application.set_lifecycle(target, Lifecycle::Trash),
    );
    trash_result.unwrap();
    measure_validation("lifecycle.trash", cardinality, || {
        assert_root_state(
            application,
            (total_rows - cardinality) as i64,
            0,
            cardinality as i64,
        )
    });
    let (undo_result, undo) = measure_mutation(
        "lifecycle.trash.undo",
        cardinality,
        application,
        store,
        library_root,
        || application.undo(),
    );
    undo_result.unwrap();
    measure_validation("lifecycle.trash.undo", cardinality, || {
        assert_root_state(application, total_rows as i64, 0, 0)
    });
    let (redo_result, redo) = measure_mutation(
        "lifecycle.trash.redo",
        cardinality,
        application,
        store,
        library_root,
        || application.redo(),
    );
    redo_result.unwrap();
    measure_validation("lifecycle.trash.redo", cardinality, || {
        assert_root_state(
            application,
            (total_rows - cardinality) as i64,
            0,
            cardinality as i64,
        )
    });
    let (restore_result, restore) = measure_mutation(
        "lifecycle.restore",
        cardinality,
        application,
        store,
        library_root,
        || {
            let restore_target = target_in_scope(target, ItemScope::Trash);
            application.set_lifecycle(&restore_target, Lifecycle::Active)
        },
    );
    restore_result.unwrap();
    measure_validation("lifecycle.restore", cardinality, || {
        assert_root_state(application, total_rows as i64, 0, 0)
    });
    for (label, value) in [
        ("lifecycle.trash", trash),
        ("lifecycle.trash.undo", undo),
        ("lifecycle.trash.redo", redo),
        ("lifecycle.restore", restore),
    ] {
        print_mutation_metric(label, cardinality, value);
    }
}

fn exercise_rating(
    application: &Arc<Application>,
    store: &Arc<Store>,
    library_root: &std::path::Path,
    target: &ItemTarget,
    cardinality: usize,
) {
    let set_patch = MediaMetadataPatch {
        rating: Some(Some(4)),
        notes: None,
        source_urls: None,
    };
    let (set_result, set) = measure_mutation(
        "rating.set",
        cardinality,
        application,
        store,
        library_root,
        || application.patch_metadata(target, &set_patch),
    );
    set_result.unwrap();
    measure_validation("rating.set", cardinality, || {
        assert_rating_cardinality(application, store, 4, cardinality as i64);
        assert_smart_cardinality(store, 4, cardinality as i64);
        let summary = selection_summary(store, target).unwrap();
        assert_eq!(summary.stats.rating_stats.min, Some(4));
        assert_eq!(summary.stats.rating_stats.max, Some(4));
        assert_eq!(summary.stats.rating_stats.shared, Some(4));
    });

    let (undo_result, undo) = measure_mutation(
        "rating.set.undo",
        cardinality,
        application,
        store,
        library_root,
        || application.undo(),
    );
    undo_result.unwrap();
    measure_validation("rating.set.undo", cardinality, || {
        assert_rating_cardinality(application, store, 4, 0);
        assert_smart_cardinality(store, 4, 0);
    });

    let (redo_result, redo) = measure_mutation(
        "rating.set.redo",
        cardinality,
        application,
        store,
        library_root,
        || application.redo(),
    );
    redo_result.unwrap();
    measure_validation("rating.set.redo", cardinality, || {
        assert_rating_cardinality(application, store, 4, cardinality as i64);
        assert_smart_cardinality(store, 4, cardinality as i64);
    });

    let clear_patch = MediaMetadataPatch {
        rating: Some(None),
        notes: None,
        source_urls: None,
    };
    let (clear_result, clear) = measure_mutation(
        "rating.clear",
        cardinality,
        application,
        store,
        library_root,
        || application.patch_metadata(target, &clear_patch),
    );
    clear_result.unwrap();
    measure_validation("rating.clear", cardinality, || {
        assert_rating_cardinality(application, store, 4, 0);
        assert_smart_cardinality(store, 4, 0);
    });

    for (label, value) in [
        ("rating.set", set),
        ("rating.set.undo", undo),
        ("rating.set.redo", redo),
        ("rating.clear", clear),
    ] {
        print_mutation_metric(label, cardinality, value);
    }
}

fn exercise_smart_folder_mutations(
    application: &Arc<Application>,
    store: &Arc<Store>,
    library_root: &std::path::Path,
    cardinalities: &[usize],
    rows: usize,
) {
    for &cardinality in cardinalities {
        let tag_predicate: SmartFolderPredicate =
            serde_json::from_value(tag_smart_folder_predicate()).unwrap();
        let minimum_size = 1_024 + (rows - cardinality + 1) as i64;
        let numeric_predicate: SmartFolderPredicate =
            serde_json::from_value(numeric_smart_folder_predicate(minimum_size)).unwrap();
        let input = CreateSmartFolderInput {
            name: format!("Matrix smart {cardinality}"),
            parent_id: None,
            predicate: tag_predicate.clone(),
            icon: None,
            color: None,
            notes: None,
            sort_field: None,
            sort_order: None,
        };
        let (created, create) = measure_mutation(
            "smart.create",
            cardinality,
            application,
            store,
            library_root,
            || application.create_smart_folder_v2(&input),
        );
        let smart_id = created.unwrap().0;
        let expected_tag = expected_creator_matches(rows);
        assert_smart_cardinality(store, smart_id, expected_tag);

        let metadata_input = CreateSmartFolderInput {
            name: format!("Matrix smart {cardinality} renamed"),
            notes: Some("metadata-only update".to_string()),
            ..input.clone()
        };
        let (metadata_result, metadata) = measure_mutation(
            "smart.metadata_update",
            cardinality,
            application,
            store,
            library_root,
            || application.update_smart_folder_v2(smart_id, &metadata_input),
        );
        metadata_result.unwrap();
        assert_smart_cardinality(store, smart_id, expected_tag);

        let predicate_input = CreateSmartFolderInput {
            predicate: numeric_predicate,
            ..metadata_input
        };
        let (predicate_result, predicate) = measure_mutation(
            "smart.predicate_update",
            cardinality,
            application,
            store,
            library_root,
            || application.update_smart_folder_v2(smart_id, &predicate_input),
        );
        predicate_result.unwrap();
        assert_smart_cardinality(store, smart_id, cardinality as i64);
        application.undo().unwrap();
        assert_smart_cardinality(store, smart_id, expected_tag);
        application.redo().unwrap();
        assert_smart_cardinality(store, smart_id, cardinality as i64);

        let (delete_result, delete) = measure_mutation(
            "smart.delete",
            cardinality,
            application,
            store,
            library_root,
            || application.delete_smart_folder_v2(smart_id),
        );
        delete_result.unwrap();
        assert_smart_cardinality(store, smart_id, 0);
        application.undo().unwrap();
        assert_smart_cardinality(store, smart_id, cardinality as i64);
        application.redo().unwrap();
        assert_smart_cardinality(store, smart_id, 0);
        for (label, value) in [
            ("smart.create", create),
            ("smart.metadata_update", metadata),
            ("smart.predicate_update", predicate),
            ("smart.delete", delete),
        ] {
            print_mutation_metric(label, cardinality, value);
        }
    }
}

fn exercise_group_operations(
    application: &Arc<Application>,
    store: &Arc<Store>,
    library_root: &std::path::Path,
    group_size: usize,
    total_rows: usize,
) {
    let members = (1..=group_size as i64).map(ItemId).collect::<Vec<_>>();
    let input = OrganizeIntoCollectionInput {
        target: ItemTarget::Explicit {
            item_ids: members.clone(),
        },
        label: Some(format!("Matrix group {group_size}")),
        winning_collection_id: None,
    };
    let (organized, organize) = measure_mutation(
        "groups.create",
        group_size,
        application,
        store,
        library_root,
        || application.organize_into_collection(input),
    );
    let collection_id = organized.unwrap().collection_id;
    assert_collection_state(application, collection_id, group_size as i64);
    assert_root_state(application, (total_rows - group_size + 1) as i64, 0, 0);

    let mut reversed = members.clone();
    reversed.reverse();
    let (reorder_result, reorder) = measure_mutation(
        "groups.reorder",
        group_size,
        application,
        store,
        library_root,
        || {
            application.reorder_collection(ReorderCollectionInput {
                collection_id,
                media_item_ids: reversed.clone(),
            })
        },
    );
    reorder_result.unwrap();
    let first_member = application
        .projections()
        .group_order(collection_id.0)
        .and_then(|order| order.first().copied())
        .unwrap();
    assert_eq!(first_member, group_size as i64);

    let (detach_result, detach) =
        measure_mutation("groups.detach", 1, application, store, library_root, || {
            application.detach_items(DetachItemsInput {
                collection_id,
                media_item_ids: vec![members[0]],
                target_lifecycle: None,
            })
        });
    detach_result.unwrap();
    assert_collection_state(application, collection_id, group_size as i64 - 1);
    assert!(application.projections().active_bitmap().contains(1));
    application.undo().unwrap();
    assert_collection_state(application, collection_id, group_size as i64);
    application.redo().unwrap();
    assert_collection_state(application, collection_id, group_size as i64 - 1);

    let (ungroup_result, ungroup) = measure_mutation(
        "groups.ungroup",
        group_size - 1,
        application,
        store,
        library_root,
        || application.ungroup_collection(collection_id),
    );
    ungroup_result.unwrap();
    assert_collection_state(application, collection_id, 0);
    assert_root_state(application, total_rows as i64, 0, 0);
    let ungroup_history = application.history_state().unwrap();
    eprintln!(
        "mutation_history operation=groups.ungroup cardinality={} undo_available={}",
        group_size - 1,
        ungroup_history.undo.is_some()
    );

    for (label, cardinality, value) in [
        ("groups.create", group_size, organize),
        ("groups.reorder", group_size, reorder),
        ("groups.detach", 1, detach),
        ("groups.ungroup", group_size - 1, ungroup),
    ] {
        print_mutation_metric(label, cardinality, value);
    }
}

fn exercise_delete(
    application: &Arc<Application>,
    store: &Arc<Store>,
    library_root: &std::path::Path,
    target: ItemTarget,
    cardinality: usize,
) {
    let history_before = application.history_state().unwrap();
    let (result, measurement) = measure_mutation(
        "items.delete",
        cardinality,
        application,
        store,
        library_root,
        || application.delete_items(&target),
    );
    assert_eq!(result.unwrap().freed_file_hashes.len(), cardinality);
    assert_eq!(application.history_state().unwrap(), history_before);
    print_mutation_metric("items.delete", cardinality, measurement);
}

fn measure_mutation<T>(
    label: &str,
    _cardinality: usize,
    application: &Arc<Application>,
    store: &Arc<Store>,
    library_root: &std::path::Path,
    operation: impl FnOnce() -> T,
) -> (T, MutationMeasurement) {
    let done = Arc::new(AtomicBool::new(false));
    let start = Arc::new(Barrier::new(FIXED_RATE_READERS + 1));
    let latencies = Arc::new(Mutex::new(Vec::<u64>::new()));
    let missed_deadlines = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut readers = Vec::with_capacity(FIXED_RATE_READERS);

    for reader_id in 0..FIXED_RATE_READERS {
        let application = Arc::clone(application);
        let store = Arc::clone(store);
        let done = Arc::clone(&done);
        let start = Arc::clone(&start);
        let latencies = Arc::clone(&latencies);
        let missed_deadlines = Arc::clone(&missed_deadlines);
        let errors = Arc::clone(&errors);
        readers.push(thread::spawn(move || {
            start.wait();
            let mut deadline = Instant::now();
            loop {
                if done.load(Ordering::Acquire) {
                    break;
                }
                deadline += FIXED_READER_PERIOD;
                let read_started = Instant::now();
                let result = fixed_rate_read(reader_id, &application, &store);
                latencies
                    .lock()
                    .unwrap()
                    .push(read_started.elapsed().as_micros() as u64);
                if let Err(error) = result {
                    errors
                        .lock()
                        .unwrap()
                        .push(format!("reader {reader_id}: {error}"));
                    break;
                }
                let now = Instant::now();
                if now < deadline {
                    thread::sleep(deadline - now);
                } else {
                    let late =
                        now.duration_since(deadline).as_nanos() / FIXED_READER_PERIOD.as_nanos();
                    missed_deadlines.fetch_add((late + 1) as u64, Ordering::Relaxed);
                    deadline += FIXED_READER_PERIOD.saturating_mul((late + 1) as u32);
                }
            }
        }));
    }

    store.reset_publication_gate_stats();
    let revision_before = store.revision().unwrap();
    let wal_before = wal_bytes(library_root);
    let history_before = history_marker(application);
    start.wait();
    thread::sleep(Duration::from_millis(1));
    let operation_started = Instant::now();
    let value = operation();
    let elapsed = operation_started.elapsed();
    done.store(true, Ordering::Release);
    for reader in readers {
        reader.join().unwrap();
    }
    let errors = errors.lock().unwrap();
    assert!(errors.is_empty(), "{label} reader failures: {errors:?}");
    let mut latencies = latencies.lock().unwrap().clone();
    latencies.sort_unstable();
    assert!(
        !latencies.is_empty(),
        "{label} fixed-rate readers did not run"
    );
    let readers = FixedRateReadStats {
        samples: latencies.len(),
        missed_deadlines: missed_deadlines.load(Ordering::Relaxed) as usize,
        p50: percentile(&latencies, 50),
        p95: percentile(&latencies, 95),
        p99: percentile(&latencies, 99),
        max: Duration::from_micros(*latencies.last().unwrap()),
    };
    let measurement = MutationMeasurement {
        elapsed,
        revision_delta: store.revision().unwrap().saturating_sub(revision_before),
        wal_growth_bytes: wal_bytes(library_root) as i64 - wal_before as i64,
        history_entry_delta: history_marker(application) - history_before,
        gate: store.publication_gate_stats(),
        readers,
    };
    (value, measurement)
}

fn fixed_rate_read(
    reader_id: usize,
    application: &Application,
    store: &Store,
) -> Result<i64, String> {
    match reader_id {
        0 => query(
            store,
            &ItemQuery {
                scope: ItemScope::All,
                filters: ItemFilters::default(),
                sort: ItemSort::default(),
            },
            ItemPageRequest::new(None, 50),
        )
        .map(|page| page.items.len() as i64),
        1 => sidebar_counts_for_application(application).map(|counts| counts.all),
        2 => query(
            store,
            &ItemQuery {
                scope: ItemScope::Folder { folder_id: 1 },
                filters: ItemFilters::default(),
                sort: ItemSort::default(),
            },
            ItemPageRequest::new(None, 50),
        )
        .map(|page| page.items.len() as i64),
        3 => query(
            store,
            &ItemQuery {
                scope: ItemScope::SmartFolder { smart_folder_id: 1 },
                filters: ItemFilters::default(),
                sort: ItemSort::default(),
            },
            ItemPageRequest::new(None, 50),
        )
        .map(|page| page.items.len() as i64),
        4 => {
            let page = query(
                store,
                &ItemQuery {
                    scope: ItemScope::All,
                    filters: ItemFilters::default(),
                    sort: ItemSort::default(),
                },
                ItemPageRequest::new(None, 1),
            )?;
            match page.items.first() {
                Some(item) => match details(application, item.item_id) {
                    Ok(details) => Ok(details.media.len() as i64),
                    Err(error) if error.contains("is not a library root") => Ok(0),
                    Err(error) => Err(error),
                },
                None => Ok(0),
            }
        }
        5 => query(
            store,
            &ItemQuery {
                scope: ItemScope::All,
                filters: ItemFilters {
                    text: Some("item-1.png".to_string()),
                    ..ItemFilters::default()
                },
                sort: ItemSort::default(),
            },
            ItemPageRequest::new(None, 10),
        )
        .map(|page| page.items.len() as i64),
        6 => query(
            store,
            &ItemQuery {
                scope: ItemScope::Inbox,
                filters: ItemFilters::default(),
                sort: ItemSort::default(),
            },
            ItemPageRequest::new(None, 50),
        )
        .map(|page| page.items.len() as i64),
        _ => query(
            store,
            &ItemQuery {
                scope: ItemScope::Trash,
                filters: ItemFilters::default(),
                sort: ItemSort::default(),
            },
            ItemPageRequest::new(None, 50),
        )
        .map(|page| page.items.len() as i64),
    }
}

fn print_mutation_metric(label: &str, cardinality: usize, metric: MutationMeasurement) {
    println!(
        "mutation_metric operation={} cardinality={} elapsed_ms={:.3} revision_delta={} wal_growth_bytes={} history_entry_delta={} gate_avg_us={} gate_max_us={} reader_samples={} reader_missed_deadlines={} reader_p50_ms={:.3} reader_p95_ms={:.3} reader_p99_ms={:.3} reader_max_ms={:.3}",
        label,
        cardinality,
        metric.elapsed.as_secs_f64() * 1_000.0,
        metric.revision_delta,
        metric.wal_growth_bytes,
        metric.history_entry_delta,
        metric.gate.average_hold_micros(),
        metric.gate.max_hold_micros,
        metric.readers.samples,
        metric.readers.missed_deadlines,
        metric.readers.p50.as_secs_f64() * 1_000.0,
        metric.readers.p95.as_secs_f64() * 1_000.0,
        metric.readers.p99.as_secs_f64() * 1_000.0,
        metric.readers.max.as_secs_f64() * 1_000.0,
    );
}

fn measure_validation(label: &str, cardinality: usize, validation: impl FnOnce()) -> Duration {
    let started = Instant::now();
    validation();
    let elapsed = started.elapsed();
    println!(
        "mutation_validation operation={} cardinality={} exact_validation_ms={:.3}",
        label,
        cardinality,
        elapsed.as_secs_f64() * 1_000.0,
    );
    elapsed
}

fn explicit_target(first_item_id: usize, count: usize) -> ItemTarget {
    ItemTarget::Explicit {
        item_ids: (first_item_id..first_item_id + count)
            .map(|item_id| ItemId(item_id as i64))
            .collect(),
    }
}

fn all_target() -> ItemTarget {
    ItemTarget::Query {
        query: ItemQuery {
            scope: ItemScope::All,
            filters: ItemFilters::default(),
            sort: ItemSort::default(),
        },
        excluded_item_ids: Vec::new(),
    }
}

fn query_target_for_tail(total_rows: usize, count: usize) -> ItemTarget {
    assert!(count > 0 && count <= total_rows);
    ItemTarget::Query {
        query: ItemQuery {
            scope: ItemScope::All,
            filters: ItemFilters {
                min_size_bytes: Some((1_024 + total_rows - count + 1) as i64),
                ..ItemFilters::default()
            },
            sort: ItemSort::default(),
        },
        excluded_item_ids: Vec::new(),
    }
}

fn range_target(first_item_id: usize, count: usize) -> ItemTarget {
    assert!(count > 0);
    ItemTarget::Range {
        query: ItemQuery {
            scope: ItemScope::All,
            filters: ItemFilters::default(),
            sort: ItemSort {
                field: ItemSortField::Size,
                direction: SortDirection::Ascending,
                random_seed: None,
            },
        },
        anchor_item_id: ItemId(first_item_id as i64),
        focus_item_id: ItemId((first_item_id + count - 1) as i64),
    }
}

fn target_in_scope(target: &ItemTarget, scope: ItemScope) -> ItemTarget {
    match target {
        ItemTarget::Explicit { item_ids } => ItemTarget::Explicit {
            item_ids: item_ids.clone(),
        },
        ItemTarget::Query {
            query,
            excluded_item_ids,
        } => ItemTarget::Query {
            query: ItemQuery {
                scope,
                filters: query.filters.clone(),
                sort: query.sort.clone(),
            },
            excluded_item_ids: excluded_item_ids.clone(),
        },
        ItemTarget::Range {
            query,
            anchor_item_id,
            focus_item_id,
        } => ItemTarget::Range {
            query: ItemQuery {
                scope,
                filters: query.filters.clone(),
                sort: query.sort.clone(),
            },
            anchor_item_id: *anchor_item_id,
            focus_item_id: *focus_item_id,
        },
    }
}

fn exact_size_sum(first_item_id: usize, count: usize) -> i64 {
    let first = first_item_id as i64;
    let last = (first_item_id + count - 1) as i64;
    count as i64 * 1_024 + (first + last) * count as i64 / 2
}

fn exact_arithmetic_size_sum(first_item_id: usize, step: usize, count: usize) -> i64 {
    if count == 0 {
        return 0;
    }
    let first = first_item_id as i64;
    let last = (first_item_id + step * (count - 1)) as i64;
    count as i64 * 1_024 + (first + last) * count as i64 / 2
}

fn multiples_in_range(first: usize, count: usize, divisor: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let last = first + count - 1;
    last / divisor - (first - 1) / divisor
}

fn assert_root_state(application: &Application, active: i64, inbox: i64, trash: i64) {
    let counts = sidebar_counts_for_application(application).unwrap();
    assert_eq!(
        (counts.all, counts.inbox, counts.trash),
        (active, inbox, trash)
    );
    assert_eq!(
        application.projections().active_bitmap().len(),
        active as u64
    );
    assert_eq!(application.projections().inbox_bitmap().len(), inbox as u64);
    assert_eq!(application.projections().trash_bitmap().len(), trash as u64);
    assert_eq!(
        query(
            application.store(),
            &ItemQuery {
                scope: ItemScope::All,
                filters: ItemFilters::default(),
                sort: ItemSort::default(),
            },
            ItemPageRequest::new(None, 1),
        )
        .unwrap()
        .visible_item_count,
        Some(active)
    );
}

fn assert_tag_cardinality(application: &Application, store: &Store, tag_name: &str, expected: i64) {
    let (namespace, subtag) = tag_name.split_once(':').unwrap();
    let value = store
        .read_snapshot(|connection| {
            connection
                .query_row(
                    "SELECT tag.tag_id, COALESCE(summary.assignment_count, 0)
                     FROM tag
                     LEFT JOIN tag_summary summary ON summary.tag_id = tag.tag_id
                     WHERE tag.namespace = ?1 AND tag.subtag = ?2",
                    rusqlite::params![namespace, subtag],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()
        })
        .unwrap();
    match value {
        Some((tag_id, count)) => {
            assert_eq!(count, expected);
            let active = application.projections().active_bitmap();
            assert_eq!(
                (&application.projections().direct_tag_bitmap(tag_id) & &active).len(),
                expected as u64
            );
            assert_eq!(
                (&application.projections().direct_tag_bitmap(tag_id) & &active).len(),
                expected as u64
            );
        }
        None => assert_eq!(expected, 0),
    }
}

fn assert_rating_cardinality(
    application: &Application,
    _store: &Store,
    rating: i64,
    expected: i64,
) {
    let page = picto_core::query_v2::query_for_application(
        application,
        &ItemQuery {
            scope: ItemScope::All,
            filters: ItemFilters {
                ratings: vec![rating],
                ..ItemFilters::default()
            },
            sort: ItemSort::default(),
        },
        ItemPageRequest::new(None, 1),
    )
    .unwrap();
    assert_eq!(page.visible_item_count, Some(expected));
}

fn assert_folder_cardinality(
    application: &Application,
    _store: &Store,
    folder_id: i64,
    expected: i64,
) {
    assert_eq!(
        application.projections().folder_bitmap(folder_id).len(),
        expected as u64
    );
}

fn assert_smart_cardinality(store: &Store, smart_folder_id: i64, expected: i64) {
    let exists: bool = store
        .read_snapshot(|connection| {
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM smart_folder WHERE smart_folder_id = ?1)",
                [smart_folder_id],
                |row| row.get(0),
            )
        })
        .unwrap();
    if !exists {
        assert_eq!(expected, 0);
        return;
    }
    let page = query(
        store,
        &ItemQuery {
            scope: ItemScope::SmartFolder { smart_folder_id },
            filters: ItemFilters::default(),
            sort: ItemSort::default(),
        },
        ItemPageRequest::new(None, 1),
    )
    .unwrap();
    assert_eq!(page.visible_item_count, Some(expected));
    let (generation_count, materialized): (i64, i64) = store
        .read_snapshot(|connection| {
            let generation = connection.query_row(
                "SELECT generation_id, member_count
                 FROM smart_folder_generation
                 WHERE smart_folder_id = ?1 AND state = 'active'
                 ORDER BY generation_id DESC LIMIT 1",
                [smart_folder_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )?;
            let materialized = connection.query_row(
                "SELECT COUNT(*) FROM smart_folder_membership
                 WHERE generation_id = ?1",
                [generation.0],
                |row| row.get(0),
            )?;
            Ok((generation.1, materialized))
        })
        .unwrap();
    assert_eq!(generation_count, expected);
    assert_eq!(materialized, expected);
}

fn assert_collection_state(
    application: &Application,
    collection_id: ItemId,
    expected_members: i64,
) {
    let root_exists: bool = application
        .store()
        .read_snapshot(|connection| {
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM library_root WHERE item_id = ?1)",
                [collection_id.0],
                |row| row.get(0),
            )
        })
        .unwrap();
    let members = application
        .projections()
        .group_order(collection_id.0)
        .map_or(0, |members| members.len() as i64);
    assert_eq!(members, expected_members);
    assert_eq!(root_exists, expected_members > 0);
}

fn assert_table_count(store: &Store, table: &str, expected: i64) {
    let count: i64 = store
        .read_snapshot(|connection| {
            connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
        })
        .unwrap();
    assert_eq!(count, expected, "unexpected row count for {table}");
}

fn history_marker(application: &Application) -> i64 {
    application
        .history_state()
        .unwrap()
        .undo
        .map(|entry| entry.entry_id)
        .unwrap_or_default()
}

fn wal_bytes(library_root: &std::path::Path) -> u64 {
    std::fs::metadata(library_root.join("library.sqlite-wal"))
        .map(|metadata| metadata.len())
        .unwrap_or_default()
}
