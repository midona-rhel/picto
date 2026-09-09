//! Query-cold verification in an established session, with independent
//! canonical SQLite oracles outside timed regions. Synthetic copies only.
use picto_library::bitmap::{self, BitmapDomain, BitmapKey};
use picto_library::database::WorkPriority;
use picto_library::predicate::{
    FilterClause, FilterExpr, SetMatchMode, SortDirection, SortField, TextField,
};
use picto_library::query::{ItemScope, RootQuery, WindowRequest};
use picto_library::query_request::QueryRequest;
use picto_library::{Library, LibraryError, Lifecycle, Rating, TagId};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn stats(values: &[f64]) -> serde_json::Value {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    json!({"n":values.len(),"samples_ms":values,"p50_ms":sorted[sorted.len().div_ceil(2)-1],
        "p95_ms":sorted[(sorted.len()*95).div_ceil(100)-1],"max_ms":sorted.last()})
}

fn text(term: &str) -> FilterExpr {
    FilterExpr::Clause(FilterClause::Text {
        field: TextField::Global,
        query: term.into(),
    })
}
fn filter(label: &str) -> FilterExpr {
    match label {
        "all" => FilterExpr::default(),
        "common" => text("landscape"),
        "rare" => text("needle"),
        "absent" => text("absent"),
        "exclude_common" => FilterExpr::Not(Box::new(text("landscape"))),
        label => FilterExpr::All(vec![
            text("landscape"),
            FilterExpr::Clause(FilterClause::Tags {
                tag_ids: vec![TagId(if label == "tag_common_text" { 1 } else { 3 })],
                mode: SetMatchMode::Any,
            }),
        ]),
    }
}

// Reference implementation of the documented seeded power-of-two permutation.
// Uses wide modular arithmetic and materialized canonical IDs, not the query
// implementation's bitmap rank/select or wrapping-u64 helper.
fn random_reference(ids: &[u32], descending: bool) -> Vec<u32> {
    if ids.len() < 2 {
        return ids.to_vec();
    }
    let digest = Sha256::digest(b"picto");
    let key = u64::from_le_bytes(digest[..8].try_into().unwrap());
    let modulus = ids.len().next_power_of_two() as u128;
    let bits = modulus.trailing_zeros();
    let a = (bits / 3).max(1);
    let b = (bits * 2 / 3).max(1);
    let mut output = Vec::with_capacity(ids.len());
    for ordinal in 0..ids.len() {
        let mut value = if descending {
            ids.len() - 1 - ordinal
        } else {
            ordinal
        } as u128;
        loop {
            value = (value + key as u128) % modulus;
            value ^= value >> a;
            value = (value * (key.rotate_left(17) | 1) as u128) % modulus;
            value ^= (value << b) % modulus;
            value = (value + key.rotate_left(41) as u128) % modulus;
            value ^= value >> b;
            if value < ids.len() as u128 {
                break;
            }
        }
        output.push(ids[value as usize]);
    }
    output
}

fn oracle(
    library: &Library,
    label: &str,
    sort: SortField,
    direction: SortDirection,
) -> picto_library::Result<(Vec<u32>, u128, u128, u64)> {
    library.database().read(WorkPriority::VisibleRead, |connection| {
        let load = |domain, key_id| bitmap::load(connection, BitmapKey {domain,key_id});
        let active = load(BitmapDomain::Lifecycle, Lifecycle::Active.bitmap_key())?;
        let tag = if label.starts_with("tag_") {
            Some(load(BitmapDomain::Tag, if label == "tag_common_text" {1} else {3})?)
        } else {None};
        let predicate = match label {
            "all" => "1",
            "rare" => "(instr(text.name,'needle')>0 OR instr(text.notes,'needle')>0 OR instr(text.urls,'needle')>0 OR instr(text.source_text,'needle')>0)",
            "absent" => "(instr(text.name,'absent')>0 OR instr(text.notes,'absent')>0 OR instr(text.urls,'absent')>0 OR instr(text.source_text,'absent')>0)",
            "exclude_common" => "NOT (instr(text.name,'landscape')>0 OR instr(text.notes,'landscape')>0 OR instr(text.urls,'landscape')>0 OR instr(text.source_text,'landscape')>0)",
            _ => "(instr(text.name,'landscape')>0 OR instr(text.notes,'landscape')>0 OR instr(text.urls,'landscape')>0 OR instr(text.source_text,'landscape')>0)",
        };
        let descending = direction == SortDirection::Descending;
        let order = match sort {
            SortField::Name => "root.name COLLATE NOCASE",
            SortField::ImportedAt => "root.imported_at_ms",
            SortField::CapturedAt => "COALESCE(root.captured_at_ms,-1)",
            SortField::TotalSize => "root.total_size_bytes",
            _ => "root.root_id",
        };
        let direction_sql = if descending && sort != SortField::Random {"DESC"} else {"ASC"};
        let mut statement = connection.prepare(&format!("SELECT root.root_id,root.media_count,root.total_size_bytes
            FROM library_root root JOIN root_fts text ON text.rowid=root.root_id WHERE {predicate}
            ORDER BY {order} {direction_sql},root.root_id {direction_sql}"))?;
        let mut rows = statement.query([])?;
        let mut ids = Vec::new(); let mut media = 0; let mut bytes = 0;
        while let Some(row) = rows.next()? {
            let id = row.get::<_,u32>(0)?;
            if !active.contains(id) || tag.as_ref().is_some_and(|tag| !tag.contains(id)) {continue;}
            ids.push(id); media += row.get::<_,i64>(1)? as u128; bytes += row.get::<_,i64>(2)? as u128;
        }
        if sort == SortField::Random { ids = random_reference(&ids, descending); }
        if sort == SortField::Rating {
            let ratings = Rating::ALL.into_iter().map(|rating| load(BitmapDomain::Rating,rating.bitmap_key())).collect::<picto_library::Result<Vec<_>>>()?;
            ids.sort_by_key(|id| (ratings.iter().position(|set|set.contains(*id)).unwrap(), *id));
            if descending {ids.reverse();}
        }
        let revision = connection.query_row("SELECT revision FROM library_meta WHERE singleton=1",[],|row| row.get::<_,i64>(0).map(|value|value as u64))?;
        Ok((ids,media,bytes,revision))
    })
}

fn matrix(mut library: Library, count: usize) -> Result<(), Box<dyn std::error::Error>> {
    // Exact optional selectors keep focused investigations reproducible. An
    // unselected full run retains the complete acceptance matrix unchanged.
    let selected = std::env::var("PICTO_BROWSE_CASES").ok().map(|value| {
        value
            .split(',')
            .map(str::to_owned)
            .collect::<std::collections::HashSet<_>>()
    });
    let mut completed = std::collections::HashSet::new();
    for sort in [
        SortField::ImportedAt,
        SortField::Name,
        SortField::TotalSize,
        SortField::Random,
        SortField::Rating,
        SortField::CapturedAt,
    ] {
        for direction in [SortDirection::Ascending, SortDirection::Descending] {
            for label in [
                "all",
                "common",
                "rare",
                "absent",
                "tag_common_text",
                "tag_rare_text",
                "exclude_common",
            ] {
                let prefix = format!(
                    "{}/{}/{label}/",
                    serde_json::to_value(sort)?.as_str().unwrap(),
                    serde_json::to_value(direction)?.as_str().unwrap()
                );
                if selected
                    .as_ref()
                    .is_some_and(|cases| !cases.iter().any(|case| case.starts_with(&prefix)))
                {
                    continue;
                }
                let mut query = RootQuery {
                    scope: ItemScope::All,
                    view: Default::default(),
                };
                query.view.filter = filter(label);
                query.view.sort.field = sort;
                query.view.sort.direction = direction;
                let (expected, media, bytes, revision) = oracle(&library, label, sort, direction)?;
                for (position, start) in [
                    ("head", 0),
                    ("quarter", expected.len() / 4),
                    ("middle", expected.len() / 2),
                    ("tail", usize::MAX),
                ] {
                    let case = format!("{prefix}{position}");
                    if selected
                        .as_ref()
                        .is_some_and(|cases| !cases.contains(&case))
                    {
                        continue;
                    }
                    completed.insert(case);
                    let mut values = Vec::new();
                    let mut stages = Vec::new();
                    for sample in 0..count {
                        library.reset_browsing_caches_for_benchmark();
                        assert_eq!(library.query_statistics().match_builds, 0);
                        let started = Instant::now();
                        let result = library.query_window_json(
                            &query,
                            &WindowRequest::at(start, 1500),
                            None,
                        )?;
                        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                        let after = library.query_statistics();
                        // Parsing and independent validation do not enter the timed region.
                        let result: serde_json::Value = serde_json::from_str(&result)?;
                        let resolved = start.min(expected.len().saturating_sub(1500));
                        assert_eq!(result["start"], json!(resolved));
                        assert_eq!(result["total"], json!(expected.len()));
                        assert_eq!(result["media_count"], json!(media));
                        assert_eq!(result["total_size_bytes"], json!(bytes));
                        assert_eq!(result["revision"], json!(revision));
                        assert_eq!(
                            result["head_items"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|item| item["root_id"].as_u64().unwrap() as u32)
                                .collect::<Vec<_>>(),
                            expected.iter().take(4).copied().collect::<Vec<_>>()
                        );
                        assert_eq!(
                            result["items"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|item| item["root_id"].as_u64().unwrap() as u32)
                                .collect::<Vec<_>>(),
                            expected[resolved..(resolved + 1500).min(expected.len())]
                        );
                        assert_eq!(after.match_builds, 1, "sample {sample} was not query-cold");
                        values.push(elapsed);
                        stages.push(after);
                    }
                    println!(
                        "CASE {}",
                        json!({"sort":sort,"direction":direction,"filter":label,"position":position,
                        "total":expected.len(),"timing":stats(&values),"stages":stages,"oracle_verified":true,
                        "scope":"established-session query/result/drawing-cache cold; includes JSON serialization; SQLite and projection data may be warm"})
                    );
                }
            }
        }
    }
    if let Some(selected) = selected {
        assert_eq!(
            completed, selected,
            "Unknown or incomplete benchmark case selectors"
        );
    }
    Ok(())
}

fn cancellation(library: Library, count: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut library = Arc::new(library);
    let mut values = Vec::new();
    for sample in 0..count {
        Arc::get_mut(&mut library)
            .unwrap()
            .reset_browsing_caches_for_benchmark();
        let worker_library = library.clone();
        let generation = sample as u64 * 2 + 1;
        let worker = std::thread::spawn(move || {
            let mut query = RootQuery {
                scope: ItemScope::All,
                view: Default::default(),
            };
            query.view.sort.field = SortField::CapturedAt;
            worker_library.query_window_json(
                &query,
                &WindowRequest::at(500_000, 1500),
                Some(&QueryRequest {
                    channel: "cancel-bench".into(),
                    generation,
                }),
            )
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while library.query_statistics().match_builds == 0
            && !worker.is_finished()
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_micros(100));
        }
        let active = library.query_statistics().match_builds > 0 && !worker.is_finished();
        let started = Instant::now();
        library.supersede_window(&QueryRequest {
            channel: "cancel-bench".into(),
            generation: generation + 1,
        })?;
        let result = worker.join().unwrap();
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        let cancelled = matches!(result, Err(LibraryError::QuerySuperseded));
        println!(
            "CANCEL {}",
            json!({"sample":sample,"active_confirmed":active,"cancelled":cancelled,"ms":elapsed})
        );
        if !active || !cancelled {
            return Err("Cancellation sample did not interrupt active query work".into());
        }
        values.push(elapsed);
    }
    println!(
        "SUMMARY {}",
        json!({"cancellation":stats(&values),"scope":"active million-item captured-date middle ordering through normal request channel; not every cancellation stage"})
    );
    Ok(())
}

fn structural_history(library: Library, count: usize) -> Result<(), Box<dyn std::error::Error>> {
    use picto_library::selection::SelectionTarget;
    use picto_library::{GroupRequest, RootId};
    library.history().clear();
    let mut original = Vec::new();
    let mut groups = Vec::new();
    let mut grouping_ms = Vec::new();
    for n in 0..count {
        let roots = vec![RootId(1 + n as u32 * 7919), RootId(2 + n as u32 * 7919)];
        for root in &roots {
            original.push(library.details(*root)?.root);
        }
        let started = Instant::now();
        let (group, _) = library.organize_into_collection(&GroupRequest {
            target: SelectionTarget::Explicit { root_ids: roots },
            cover_root_id: RootId(1 + n as u32 * 7919),
            winning_collection_id: None,
            name: Some(format!("Synthetic history {n}")),
            notes: None,
            modified_at_ms: 1_700_000_000_000,
        })?;
        grouping_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        groups.push(group);
    }
    let retained = library.history().state();
    assert_eq!(
        retained.entries, count,
        "Every group must retain its undo entry in a million-item library"
    );
    assert!(
        retained.bytes < count * 20_000,
        "Small groups must not retain library-wide projections"
    );
    let mut undo_ms = Vec::new();
    for _ in 0..count {
        let started = Instant::now();
        assert!(library.undo()?.is_some());
        undo_ms.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    assert_eq!(library.counts()?.all, 1_000_000);
    for root in &original {
        assert_eq!(&library.details(root.root_id)?.root, root);
    }
    let mut redo_ms = Vec::new();
    for _ in 0..count {
        let started = Instant::now();
        assert!(library.redo()?.is_some());
        redo_ms.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    assert_eq!(library.counts()?.all, 1_000_000 - count as u64);
    for group in groups {
        assert_eq!(library.details(group)?.root.media_count, 2);
    }
    for _ in 0..count {
        assert!(library.undo()?.is_some());
    }
    for root in &original {
        assert_eq!(&library.details(root.root_id)?.root, root);
    }
    println!(
        "HISTORY {}",
        json!({"group":stats(&grouping_ms),"undo":stats(&undo_ms),"redo":stats(&redo_ms),
        "retained_entries":retained.entries,"retained_history_bytes":retained.bytes,"canonical_roots_verified":original.len(),
        "scope":"Thirty independent two-root groups followed by complete undo/redo/undo; canonical root records verified outside timed regions"})
    );
    Ok(())
}

/// Keep the matching set fixed with an existing tag while unrelated active
/// imports publish new snapshots. The SQL oracle is collected before timing;
/// new assets have no tags and therefore cannot change this expected order.
fn import_responsiveness(library: Library, count: usize) -> Result<(), Box<dyn std::error::Error>> {
    use picto_library::{ImmutableMediaFacts, PreparedImport};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Barrier;

    let (expected, media, bytes, _) = oracle(
        &library,
        "tag_common_text",
        SortField::Name,
        SortDirection::Ascending,
    )?;
    let query = RootQuery {
        scope: ItemScope::All,
        view: picto_library::predicate::ViewQuerySpec {
            filter: filter("tag_common_text"),
            sort: picto_library::predicate::ItemSort {
                field: SortField::Name,
                direction: SortDirection::Ascending,
                random_seed: None,
            },
        },
    };
    let stop = AtomicBool::new(false);
    let imported = AtomicUsize::new(0);
    let barrier = Barrier::new(2);
    let mut values = Vec::new();
    let mut revisions = Vec::new();
    let mut overlaps = Vec::new();
    let initial_revision = library.projections().snapshot().revision;
    std::thread::scope(|scope| -> Result<(), Box<dyn std::error::Error>> {
        let writer = scope.spawn(|| -> picto_library::Result<()> {
            barrier.wait();
            let deadline = Instant::now() + Duration::from_secs(30);
            for batch in 0..2000 {
                if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                    break;
                }
                let inputs = (0..48)
                    .map(|slot| {
                        let index = batch * 48 + slot;
                        PreparedImport {
                            stable_key: format!("concurrent-probe-{index}"),
                            media_name: format!("concurrent-probe-{index}.png"),
                            file_path: format!("/synthetic-only/concurrent-probe-{index}.png"),
                            facts: ImmutableMediaFacts {
                                mime: "image/png".into(),
                                size_bytes: 1024,
                                width: Some(800),
                                height: Some(600),
                                duration_ms: None,
                                frame_count: Some(1),
                                content_hash: format!("concurrent-probe-{index}"),
                                perceptual_hash: None,
                                palette: Vec::new(),
                            },
                            lifecycle: Lifecycle::Active,
                            rating: Rating::Unrated,
                            notes: None,
                            tags: Vec::new(),
                            folders: Vec::new(),
                            source_urls: Vec::new(),
                            source_identity: None,
                            imported_at_ms: 1_800_000_000_000 + index as i64,
                            captured_at_ms: None,
                        }
                    })
                    .collect::<Vec<_>>();
                library.ingest_batch(&inputs)?;
                library.settle_fts(48)?;
                imported.fetch_add(48, Ordering::Release);
            }
            Ok(())
        });
        barrier.wait();
        let reads = (|| -> Result<(), Box<dyn std::error::Error>> {
            for sample in 0..count {
                let start = (sample * 104729) % expected.len().saturating_sub(1500).max(1);
                let before = imported.load(Ordering::Acquire);
                let active = !writer.is_finished();
                let started = Instant::now();
                let encoded = library.query_window_json(
                    &query,
                    &WindowRequest::at(start, 1500),
                    Some(&QueryRequest {
                        channel: "import-responsiveness".into(),
                        generation: sample as u64 + 1,
                    }),
                )?;
                let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                let after = imported.load(Ordering::Acquire);
                // Decode and canonical-oracle assertions are outside timing.
                let result: serde_json::Value = serde_json::from_str(&encoded)?;
                assert_eq!(result["total"].as_u64(), Some(expected.len() as u64));
                assert_eq!(result["media_count"].as_u64(), Some(media as u64));
                assert_eq!(result["total_size_bytes"].as_u64(), Some(bytes as u64));
                let ids = |field: &str| {
                    result[field]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|item| item["root_id"].as_u64().unwrap() as u32)
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    ids("items"),
                    expected[start..(start + 1500).min(expected.len())]
                );
                assert_eq!(ids("head_items"), expected[..4.min(expected.len())]);
                let revision = result["revision"].as_u64().unwrap();
                assert!(revision >= *revisions.last().unwrap_or(&initial_revision));
                revisions.push(revision);
                values.push(elapsed);
                overlaps.push(json!({"sample":sample,"writer_active_at_start":active,
                    "completed_imports_before":before,"completed_imports_after":after}));
            }
            Ok(())
        })();
        stop.store(true, Ordering::Release);
        writer.join().map_err(|_| "Import worker panicked")??;
        reads
    })?;
    assert!(
        imported.load(Ordering::Acquire) > 0,
        "No import publication overlapped the read sequence"
    );
    assert!(
        revisions.last().unwrap() > &initial_revision,
        "Reads never observed a writer publication"
    );
    assert!(
        overlaps
            .iter()
            .all(|sample| sample["writer_active_at_start"] == true),
        "Writer ended early; do not classify every read as concurrent"
    );
    let gate = library.database().publication_gate_stats();
    println!(
        "SUMMARY {}",
        json!({"import_responsiveness":stats(&values),
        "imported":imported.load(Ordering::Acquire),"revisions":revisions,"overlaps":overlaps,
        "oracle_verified":true,"publication_gate_p95_upper_bound_us":gate.p95_micros,
        "publication_gate_max_us":gate.max_micros,
        "scope":"Million-root tag-plus-text alphabetical arbitrary windows during continuous 48-item imports and 48-item FTS batches; first request query-cold, later membership may be reused; includes serialization, excludes media decoding and renderer"})
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("Expected isolated synthetic SQLite path")?;
    let mode = args.next().ok_or("Expected matrix or cancellation")?;
    let count: usize = args.next().unwrap_or_else(|| "30".into()).parse()?;
    if !(1..=100).contains(&count) {
        return Err("Use 1–100 samples".into());
    }
    let library = Library::open(path)?;
    if library.projections().snapshot().active().len() != 1_000_000 {
        return Err("Expected million active synthetic roots".into());
    }
    match mode.as_str() {
        "matrix" => matrix(library, count),
        "cancellation" => cancellation(library, count),
        "history" => structural_history(library, count),
        "imports" => import_responsiveness(library, count),
        _ => Err("Unknown verification mode".into()),
    }
}
