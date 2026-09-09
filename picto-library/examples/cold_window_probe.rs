//! Queries against an explicitly supplied synthetic library. Opening can migrate
//! or recover it; callers must supply an isolated copy, never the source fixture.
use picto_library::predicate::{SortDirection, SortField};
use picto_library::query::{ItemScope, RootQuery, WindowRequest};
use picto_library::Library;
use serde_json::json;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("Expected a library.sqlite path")?;
    let startup_only = std::env::args().nth(2).as_deref() == Some("--startup-only");
    for field in [SortField::ImportedAt, SortField::Name, SortField::TotalSize] {
        for run in 0..3 {
            let opened = Instant::now();
            let library = Library::open(&path)?;
            let open_ms = opened.elapsed().as_secs_f64() * 1000.0;
            println!(
                "OPEN {}",
                json!({"sort":field,"run":run,"open_ms":open_ms,
                "projection":library.projections().load_statistics()})
            );
            let mut query = RootQuery {
                scope: ItemScope::All,
                view: Default::default(),
            };
            query.view.sort.field = field;
            query.view.sort.direction = SortDirection::Descending;
            let head_started = Instant::now();
            let head = library.query_window(
                &query,
                &WindowRequest::at(0, if startup_only { 1500 } else { 500 }),
            )?;
            if startup_only {
                println!(
                    "STARTUP {}",
                    json!({"open_ms":open_ms,
                    "first_window_ms":head_started.elapsed().as_secs_f64()*1000.0,
                    "projection":library.projections().load_statistics(),
                    "total":head.page.total,"items":head.page.items.len(),
                    "revision":head.page.revision})
                );
                return Ok(());
            }
            let before = library.query_statistics();
            let started = Instant::now();
            let tail = library.query_window(&query, &WindowRequest::at(usize::MAX, 1500))?;
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            let after = library.query_statistics();
            assert_eq!(head.page.total, tail.page.total);
            assert_eq!(tail.start, (tail.page.total as usize).saturating_sub(1500));
            println!(
                "COLD {}",
                json!({"sort":field,"run":run,"total":tail.page.total,
                "ms":elapsed,"order_builds":after.order_builds-before.order_builds,
                "order_ms":(after.order_us-before.order_us) as f64/1000.0,
                "summary_ms":(after.summary_us-before.summary_us) as f64/1000.0,
                "first_id":tail.page.items.first().map(|i|i.root_id),
                "last_id":tail.page.items.last().map(|i|i.root_id)})
            );
        }
    }
    Ok(())
}
