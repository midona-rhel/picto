//! Isolated query/serialization benchmark; fixture setup is bulk SQL, not an import benchmark.
//! cargo run --release -p picto_library --example grid_query_scale_probe -- 10000 100000 1000000
use std::collections::HashSet;
use std::time::Instant;

use picto_library::bitmap::{self, BitmapDomain, BitmapKey};
use picto_library::predicate::{
    FilterClause, FilterExpr, ItemSort, SetMatchMode, SortDirection, SortField, TextField,
    ViewQuerySpec,
};
use picto_library::query::{ItemScope, PageRequest, RootQuery};
use picto_library::{LabColor, Library, TagId};
use roaring::RoaringBitmap;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let counts = std::env::args()
        .skip(1)
        .map(|s| s.parse::<u32>())
        .collect::<Result<Vec<_>, _>>()?;
    assert!(!counts.is_empty() && counts.iter().all(|n| *n > 0 && *n <= 1_000_000));
    let mut reports = Vec::new();
    for count in counts {
        let directory = tempfile::Builder::new()
            .prefix("picto-grid-scale-")
            .tempdir()?;
        let path = directory.path().join("fixture.sqlite");
        eprintln!("SETUP count={count} path={}", path.display());
        let started = Instant::now();
        seed(&path, count)?;
        let seed_ms = elapsed(started);
        let started = Instant::now();
        let library = Library::open(&path)?;
        let open_ms = elapsed(started);
        let active = count as u64 - (count / 20) as u64 - count.div_ceil(20) as u64;
        assert_eq!(library.counts()?.all, active);
        let mut cases = Vec::new();
        for (label, query) in queries() {
            let first_started = Instant::now();
            let first = library.query(
                &query,
                &PageRequest {
                    limit: 500,
                    cursor: None,
                },
            )?;
            let first_query_ms = elapsed(first_started);
            let started = Instant::now();
            let bytes = serde_json::to_vec(&first)?;
            let first_serialize_ms = elapsed(started);
            assert!(first.items.len() <= 500);
            let first_ids = first
                .items
                .iter()
                .map(|item| item.root_id.0)
                .collect::<HashSet<_>>();
            assert_eq!(first_ids.len(), first.items.len());
            let mut query_ms = Vec::new();
            let mut serialize_ms = Vec::new();
            for _ in 0..10 {
                let started = Instant::now();
                let page = library.query(
                    &query,
                    &PageRequest {
                        limit: 500,
                        cursor: None,
                    },
                )?;
                query_ms.push(elapsed(started));
                assert_eq!(page.total, first.total);
                assert_eq!(
                    page.items
                        .iter()
                        .map(|item| item.root_id.0)
                        .collect::<HashSet<_>>(),
                    first_ids
                );
                let started = Instant::now();
                std::hint::black_box(serde_json::to_vec(&page)?);
                serialize_ms.push(elapsed(started));
            }
            let mut cursor = first.next_cursor.clone();
            let mut seen = first_ids;
            let mut walk_ms = Vec::new();
            let max_pages = if label == "random" || label.starts_with("text") {
                5
            } else {
                100
            };
            for _ in 1..max_pages {
                let Some(next) = cursor.take() else { break };
                let started = Instant::now();
                let page = library.query(
                    &query,
                    &PageRequest {
                        limit: 500,
                        cursor: Some(next),
                    },
                )?;
                walk_ms.push(elapsed(started));
                for item in &page.items {
                    assert!(
                        seen.insert(item.root_id.0),
                        "duplicate across pages in {label}"
                    );
                }
                cursor = page.next_cursor;
            }
            let row = json!({"case":label,"matches":first.total,"returned":first.items.len(),
                "first_query_ms":first_query_ms,"first_serialize_ms":first_serialize_ms,"payload_bytes":bytes.len(),
                "query_ms":stats(&query_ms),"serialize_ms":stats(&serialize_ms),
                "later_page_ms":stats(&walk_ms),"last_page_ms":walk_ms.last(),"pages_walked":walk_ms.len()+1});
            println!("CASE count={count} {row}");
            cases.push(row);
        }
        let snapshot_bytes = library.projections().snapshot().estimated_bytes();
        let started = Instant::now();
        library.mime_facets(&RootQuery {
            scope: ItemScope::All,
            view: ViewQuerySpec::default(),
        })?;
        let facets_ms = elapsed(started);
        let report = json!({"roots":count,"active_roots":active,"seed_ms":seed_ms,"open_rebuild_ms":open_ms,
            "database_bytes":std::fs::metadata(&path)?.len(),"projection_estimated_bytes":snapshot_bytes,
            "mime_facets_ms":facets_ms,"cases":cases});
        println!("SUMMARY {report}");
        reports.push(report);
        drop(library);
        directory.close()?;
        eprintln!("CLEANED count={count}");
    }
    println!(
        "REPORT {}",
        json!({"profile":"release","page_size":500,"samples_per_case":10,
        "fixture":"standalone roots; 90% active; mixed types; six palette colors; synthetic tags; no media bytes",
        "cache":"no Electron serialized-response cache; SQLite OS caches warm; text match cache remains production behavior",
        "results":reports})
    );
    Ok(())
}

fn elapsed(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}
fn stats(values: &[f64]) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    json!({"n":sorted.len(),"median":sorted[sorted.len()/2],"p95":sorted[((sorted.len()*95).div_ceil(100)-1).min(sorted.len()-1)],
        "min":sorted[0],"max":sorted[sorted.len()-1],"mean":sorted.iter().sum::<f64>()/sorted.len() as f64})
}

pub fn queries() -> Vec<(&'static str, RootQuery)> {
    let base = RootQuery {
        scope: ItemScope::All,
        view: ViewQuerySpec::default(),
    };
    let mut queries = vec![("all_imported", base.clone())];
    for (label, field) in [
        ("name", SortField::Name),
        ("size", SortField::TotalSize),
        ("random", SortField::Random),
    ] {
        let mut query = base.clone();
        query.view.sort = ItemSort {
            field,
            direction: SortDirection::Descending,
            random_seed: Some("scale-fixed-seed".into()),
        };
        queries.push((label, query));
    }
    let mut inbox = base.clone();
    inbox.scope = ItemScope::Inbox;
    queries.push(("inbox", inbox));
    for (label, filter) in [
        (
            "mime_png",
            FilterClause::Mime {
                values: vec!["image/png".into()],
                families: vec![],
            },
        ),
        (
            "tag_common",
            FilterClause::Tags {
                tag_ids: vec![TagId(1)],
                mode: SetMatchMode::Any,
            },
        ),
        (
            "tag_one_percent",
            FilterClause::Tags {
                tag_ids: vec![TagId(2)],
                mode: SetMatchMode::Any,
            },
        ),
        (
            "tag_rare",
            FilterClause::Tags {
                tag_ids: vec![TagId(3)],
                mode: SetMatchMode::Any,
            },
        ),
        (
            "text_rare",
            FilterClause::Text {
                field: TextField::Name,
                query: "needle".into(),
            },
        ),
        (
            "text_common",
            FilterClause::Text {
                field: TextField::Name,
                query: "landscape".into(),
            },
        ),
    ] {
        let mut query = base.clone();
        query.view.filter = FilterExpr::Clause(filter);
        queries.push((label, query));
    }
    queries
}

pub struct SampleAsset {
    pub hash: String,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub size: u64,
}

pub fn seed(path: &std::path::Path, count: u32) -> Result<(), Box<dyn std::error::Error>> {
    seed_with_assets(path, count, &[])
}

pub fn seed_with_assets(
    path: &std::path::Path,
    count: u32,
    assets: &[SampleAsset],
) -> Result<(), Box<dyn std::error::Error>> {
    assert!(!path.exists());
    let mut connection = Connection::open(path)?;
    connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    picto_library::schema::create(&mut connection)?;
    let palettes = (0..256)
        .map(|i| {
            serde_json::to_string(
                &(0..6)
                    .map(|j| LabColor {
                        l: ((i * 17 + j * 11) % 100) as f32,
                        a: ((i * 13 + j * 23) % 128) as f32 - 64.0,
                        b: ((i * 19 + j * 7) % 128) as f32 - 64.0,
                        weight: 1.0 / 6.0,
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut lifecycle = [
        RoaringBitmap::new(),
        RoaringBitmap::new(),
        RoaringBitmap::new(),
    ];
    let mut ratings = std::array::from_fn::<_, 6, _>(|_| RoaringBitmap::new());
    let mut tags = std::array::from_fn::<_, 3, _>(|_| RoaringBitmap::new());
    let transaction = connection.transaction()?;
    transaction.execute("INSERT INTO tag_namespace(namespace_id, stable_key, display_name) VALUES (1,'bench-ns','benchmark')", [])?;
    for id in 1..=3 {
        transaction.execute(
            "INSERT INTO tag_definition VALUES (?1, ?2, 1, ?2)",
            params![id, format!("tag-{id}")],
        )?;
    }
    {
        let mut item = transaction.prepare("INSERT INTO library_item VALUES (?1, ?2, 1)")?;
        let mut file = transaction.prepare("INSERT INTO media_file(file_id,content_hash,file_path,mime,size_bytes,width,height,duration_ms,frame_count,palette_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)")?;
        let mut media = transaction
            .prepare("INSERT INTO media_item(media_id,media_name,file_id) VALUES (?1,?2,?3)")?;
        let mut root = transaction.prepare("INSERT INTO library_root(root_id,name,cover_media_id,imported_at_ms,captured_at_ms,modified_at_ms,media_count,total_size_bytes) VALUES (?1,?2,?1,?3,?4,?3,1,?5)")?;
        for id in 1..=count {
            let mix = id.wrapping_mul(2654435761);
            let name = format!(
                "{}-{:08}-{:08}",
                if id % 997 == 0 {
                    "needle"
                } else if mix % 2 == 0 {
                    "landscape"
                } else {
                    "portrait"
                },
                mix,
                id
            );
            let asset = (!assets.is_empty()).then(|| &assets[(id as usize - 1) % assets.len()]);
            let file_id = if assets.is_empty() {
                id
            } else {
                (id - 1) % assets.len() as u32 + 1
            };
            let mime = if asset.is_some() {
                "image/jpeg"
            } else {
                match (mix >> 8) % 10 {
                    0..=3 => "image/jpeg",
                    4..=6 => "image/png",
                    7 => "video/webm",
                    8 => "audio/mpeg",
                    _ => "application/x-shockwave-flash",
                }
            };
            let size = asset
                .map(|asset| asset.size as i64)
                .unwrap_or(100_000 + mix as i64 % 50_000_000);
            item.execute(params![id, format!("fixture-{id}")])?;
            if asset.is_none() || id as usize <= assets.len() {
                file.execute(params![
                    file_id,
                    asset
                        .map(|asset| asset.hash.clone())
                        .unwrap_or_else(|| format!("{id:064x}")),
                    asset
                        .map(|asset| asset.path.clone())
                        .unwrap_or_else(|| format!("fixture/{id}")),
                    mime,
                    size,
                    asset.map(|asset| asset.width).unwrap_or(800 + mix % 2048),
                    asset
                        .map(|asset| asset.height)
                        .unwrap_or(600 + (mix >> 10) % 1536),
                    if mime.starts_with("video/") || mime.starts_with("audio/") {
                        Some((mix % 300_000 + 1000) as i64)
                    } else {
                        None
                    },
                    if mime.starts_with("video/") { 300 } else { 1 },
                    &palettes[(mix % 256) as usize]
                ])?;
            }
            media.execute(params![id, name, file_id])?;
            root.execute(params![
                id,
                name,
                1_700_000_000_000i64 + id as i64,
                1_600_000_000_000i64 + mix as i64,
                size
            ])?;
            lifecycle[if asset.is_some() {
                0
            } else {
                match id % 20 {
                    0 => 2,
                    1 => 1,
                    _ => 0,
                }
            }]
            .insert(id);
            ratings[(mix % 6) as usize].insert(id);
            if mix % 2 == 0 {
                tags[0].insert(id);
            }
            if mix % 100 == 0 {
                tags[1].insert(id);
            }
            if id % 997 == 0 {
                tags[2].insert(id);
            }
            if id % 100_000 == 0 {
                eprintln!("SEED {id}/{count}");
            }
        }
    }
    for (i, values) in lifecycle.iter().enumerate() {
        bitmap::replace(
            &transaction,
            1,
            BitmapKey {
                domain: BitmapDomain::Lifecycle,
                key_id: i as u32 + 1,
            },
            values,
        )?;
    }
    for (i, values) in ratings.iter().enumerate() {
        bitmap::replace(
            &transaction,
            1,
            BitmapKey {
                domain: BitmapDomain::Rating,
                key_id: i as u32,
            },
            values,
        )?;
    }
    for (i, values) in tags.iter().enumerate() {
        bitmap::replace(
            &transaction,
            1,
            BitmapKey {
                domain: BitmapDomain::Tag,
                key_id: i as u32 + 1,
            },
            values,
        )?;
    }
    transaction.execute("UPDATE library_meta SET next_local_id=?1", [count + 1])?;
    eprintln!("FTS count={count}");
    transaction.execute("INSERT INTO root_fts(rowid,name,notes,urls,source_text) SELECT root_id,lower(name),'','','' FROM library_root", [])?;
    transaction.commit()?;
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    let violation: Option<String> = connection
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |row| row.get(0))?
        .next()
        .transpose()?;
    assert!(
        violation.is_none(),
        "fixture foreign-key violation: {violation:?}"
    );
    Ok(())
}
