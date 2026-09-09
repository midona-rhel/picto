//! Every measured request uses a reopened library: no cached matches or orders.
//! This is query-cold in one process, NOT process-cold or OS-cold. Prepare a
//! valid clean-close checkpoint first; migration/recovery is timed separately.
use picto_library::bitmap::{self, BitmapDomain, BitmapKey};
use picto_library::predicate::{FilterClause, FilterExpr, SortField, TextField};
use picto_library::query::{ItemScope, RootQuery, WindowRequest};
use picto_library::{Library, Lifecycle};
use serde_json::json;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("Expected synthetic library.sqlite")?;
    let output = args.next();
    let prepare = Instant::now();
    let prepared = Library::open(&path)?;
    let recovery_open_ms = prepare.elapsed().as_secs_f64() * 1000.0;
    let checkpoint = Instant::now();
    let checkpoint_bytes = prepared.write_projection_checkpoint()?;
    prepared.database().checkpoint_wal()?;
    println!(
        "PREPARE {}",
        json!({
            "purpose":"synthetic fixture migration/recovery and clean-close persistence, excluded from normal reopen timings",
            "recovery_open_ms":recovery_open_ms,
            "checkpoint_ms":checkpoint.elapsed().as_secs_f64()*1000.0,
            "checkpoint_bytes":checkpoint_bytes,
        })
    );
    drop(prepared);
    let mut samples = Vec::new();
    for field in [
        SortField::ImportedAt,
        SortField::TotalSize,
        SortField::Name,
        SortField::Random,
    ] {
        for needle in ["landscape", "needle", "000", "absent"] {
            for start in [0, 250_000, usize::MAX] {
                let opened = Instant::now();
                let library = Library::open(&path)?;
                let open_ms = opened.elapsed().as_secs_f64() * 1000.0;
                let text_bytes = library
                    .projections()
                    .snapshot()
                    .text_index
                    .estimated_bytes();
                let mut query = RootQuery {
                    scope: ItemScope::All,
                    view: Default::default(),
                };
                query.view.sort.field = field;
                query.view.filter = FilterExpr::Clause(FilterClause::Text {
                    field: TextField::Global,
                    query: needle.into(),
                });
                let before = library.query_statistics();
                let started = Instant::now();
                let result = library.query_window(&query, &WindowRequest::at(start, 1500))?;
                let ms = started.elapsed().as_secs_f64() * 1000.0;
                let after = library.query_statistics();
                assert_eq!(
                    result.start,
                    start.min((result.page.total as usize).saturating_sub(1500))
                );
                assert_eq!(
                    result.page.items.len(),
                    (result.page.total as usize).min(1500)
                );
                // Independent canonical ordering oracle, outside the timed request.
                if field != SortField::Random {
                    let (expected, media_count, size, revision) = library.database().read(
                        picto_library::database::WorkPriority::VisibleRead,
                        |connection| {
                            // Read canonical lifecycle partitions and the settled text
                            // corpus directly. Do not reuse candidate matching, cached
                            // counts or projection state to validate their own results.
                            let active = bitmap::load(
                                connection,
                                BitmapKey {
                                    domain: BitmapDomain::Lifecycle,
                                    key_id: Lifecycle::Active.bitmap_key(),
                                },
                            )?;
                            let direction = if query.view.sort.direction
                                == picto_library::predicate::SortDirection::Ascending
                            {
                                "ASC"
                            } else {
                                "DESC"
                            };
                            let sort = match field {
                                SortField::Name => "root.name COLLATE NOCASE",
                                SortField::TotalSize => "root.total_size_bytes",
                                _ => "root.imported_at_ms",
                            };
                            let mut statement = connection.prepare(&format!(
                                "SELECT root.root_id, root.media_count, root.total_size_bytes
                            FROM library_root root JOIN root_fts text ON text.rowid = root.root_id
                            WHERE (text.name LIKE ?1 AND instr(text.name, ?2) > 0)
                               OR (text.notes LIKE ?1 AND instr(text.notes, ?2) > 0)
                               OR (text.urls LIKE ?1 AND instr(text.urls, ?2) > 0)
                               OR (text.source_text LIKE ?1 AND instr(text.source_text, ?2) > 0)
                            ORDER BY {sort} {direction}, root.root_id {direction}"
                            ))?;
                            let lowered = needle.to_lowercase();
                            let mut rows = statement.query([format!("%{lowered}%"), lowered])?;
                            let mut ids = Vec::new();
                            let mut media_count = 0_u128;
                            let mut size = 0_u128;
                            while let Some(row) = rows.next()? {
                                let id = row.get::<_, u32>(0)?;
                                if !active.contains(id) {
                                    continue;
                                }
                                ids.push(id);
                                media_count += row.get::<_, i64>(1)? as u128;
                                size += row.get::<_, i64>(2)? as u128;
                            }
                            let revision = connection.query_row(
                                "SELECT revision FROM library_meta WHERE singleton=1",
                                [],
                                |row| row.get::<_, i64>(0).map(|value| value as u64),
                            )?;
                            Ok((ids, media_count, size, revision))
                        },
                    )?;
                    assert_eq!(result.page.total as usize, expected.len());
                    assert_eq!(result.page.media_count, media_count);
                    assert_eq!(result.page.total_size_bytes, size);
                    assert_eq!(result.page.revision, revision);
                    assert_eq!(
                        result
                            .head_items
                            .iter()
                            .map(|item| item.root_id.0)
                            .collect::<Vec<_>>(),
                        expected.iter().take(4).copied().collect::<Vec<_>>()
                    );
                    assert_eq!(
                        result
                            .page
                            .items
                            .iter()
                            .map(|item| item.root_id.0)
                            .collect::<Vec<_>>(),
                        expected[result.start..(result.start + 1500).min(expected.len())]
                    );
                }
                let sample = json!({"sort":field,"query":needle,"requested_start":start,"start":result.start,
                    "total":result.page.total,"items":result.page.items.len(),"ms":ms,"open_ms":open_ms,
                    "text_bytes":text_bytes,
                    "name_bytes":library.projections().snapshot().name_index.estimated_bytes(),
                    "cold_kind":"query-cold reopened library with a valid clean-close checkpoint; OS cache uncontrolled",
                    "sql_order_verified":field != SortField::Random,
                    "match_ms":(after.match_us-before.match_us) as f64/1000.0,
                    "order_ms":(after.order_us-before.order_us) as f64/1000.0,
                    "summary_ms":(after.summary_us-before.summary_us) as f64/1000.0});
                println!("COLD_SEARCH {sample}");
                samples.push(sample);
            }
        }
    }
    if let Some(output) = output {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?;
        file.write_all(&serde_json::to_vec_pretty(&samples)?)?;
    }
    Ok(())
}
