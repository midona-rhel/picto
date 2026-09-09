//! Fresh-library query profiling. PRAGMAs apply only to this probe's leased reader.
use picto_library::database::WorkPriority;
use picto_library::predicate::{FilterClause, FilterExpr, SortField, TextField};
use picto_library::query::{ItemScope, RootQuery, WindowRequest};
use picto_library::Library;
use rusqlite::{ffi, Connection};
use serde_json::json;
use std::time::Instant;

fn status(connection: &Connection, code: i32, reset: bool) -> i32 {
    let (mut current, mut high) = (0, 0);
    // SAFETY: this thread owns the reader lease; SQLite writes two valid integers.
    let result = unsafe {
        ffi::sqlite3_db_status(
            connection.handle(),
            code,
            &mut current,
            &mut high,
            i32::from(reset),
        )
    };
    assert_eq!(result, ffi::SQLITE_OK);
    current
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("Expected synthetic library.sqlite")?;
    let output = args.next();
    let mut samples = Vec::new();
    for mode in ["mmap", "default", "mmap", "cache", "default"] {
        for (field, term, start) in [
            (SortField::Name, "portrait", 250_000),
            (SortField::Name, "landscape", usize::MAX),
            (SortField::Random, "landscape", 250_000),
        ] {
            let library = Library::open(&path)?;
            let settings = library
                .database()
                .read(WorkPriority::VisibleRead, |connection| {
                    match mode {
                        "mmap" => connection.execute_batch("PRAGMA mmap_size=1073741824")?,
                        "cache" => connection
                            .execute_batch("PRAGMA mmap_size=0; PRAGMA cache_size=-16384")?,
                        _ => connection.execute_batch("PRAGMA mmap_size=0")?,
                    }
                    let mmap: i64 = connection.query_row("PRAGMA mmap_size", [], |r| r.get(0))?;
                    let cache: i64 = connection.query_row("PRAGMA cache_size", [], |r| r.get(0))?;
                    status(connection, ffi::SQLITE_DBSTATUS_CACHE_HIT, true);
                    status(connection, ffi::SQLITE_DBSTATUS_CACHE_MISS, true);
                    Ok(json!({"mmap":mmap,"cache_kib":cache}))
                })?;
            let mut query = RootQuery {
                scope: ItemScope::All,
                view: Default::default(),
            };
            query.view.sort.field = field;
            query.view.filter = FilterExpr::Clause(FilterClause::Text {
                field: TextField::Global,
                query: term.into(),
            });
            let before = library.query_statistics();
            let started = Instant::now();
            let result = library.query_window(&query, &WindowRequest::at(start, 1500))?;
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            let after = library.query_statistics();
            let pages = library
                .database()
                .read(WorkPriority::VisibleRead, |connection| {
                    Ok(json!({
                        "hits":status(connection,ffi::SQLITE_DBSTATUS_CACHE_HIT,false),
                        "misses":status(connection,ffi::SQLITE_DBSTATUS_CACHE_MISS,false),
                        "cache_bytes":status(connection,ffi::SQLITE_DBSTATUS_CACHE_USED,false),
                    }))
                })?;
            assert_eq!(result.page.items.len(), 1500);
            let sample = json!({"mode":mode,"sort":field,"term":term,"start":result.start,"ms":ms,
                "match_ms":(after.match_us-before.match_us) as f64/1000.0,
                "order_ms":(after.order_us-before.order_us) as f64/1000.0,
                "summary_ms":(after.summary_us-before.summary_us) as f64/1000.0,"settings":settings,"pages":pages});
            println!("COLD_READ_PROFILE {sample}");
            samples.push(sample);
        }
    }
    if let Some(output) = output {
        std::fs::write(output, serde_json::to_vec_pretty(&samples)?)?;
    }
    Ok(())
}
