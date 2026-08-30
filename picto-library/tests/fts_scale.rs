//! Scale check for text search. Ignored by default; run explicitly with
//! `cargo test --release --test fts_scale -- --ignored --nocapture`.

use std::time::Instant;

use picto_library::predicate::{FilterClause, FilterExpr, ItemSort, TextField, ViewQuerySpec};
use picto_library::query::{ItemScope, PageRequest, RootQuery};
use picto_library::{ImmutableMediaFacts, LabColor, Library, Lifecycle, PreparedImport, Rating};
use tempfile::TempDir;

const DEFAULT_ITEMS: usize = 200_000;
const BATCH: usize = 48;
const SETTLE_BATCH: usize = 2_000;

const WORDS: &[&str] = &[
    "sunset",
    "dragon",
    "forest",
    "portrait",
    "sketch",
    "render",
    "castle",
    "ocean",
    "winter",
    "summer",
    "night",
    "study",
    "concept",
    "banner",
    "poster",
    "avatar",
    "icon",
    "logo",
    "photo",
    "scan",
    "newgrounds",
    "artstation",
    "commission",
    "wallpaper",
    "landscape",
    "character",
    "animation",
    "pixel",
    "vector",
    "gradient",
    "shadow",
    "light",
    "storm",
    "river",
    "mountain",
    "cyber",
    "retro",
    "space",
    "galaxy",
    "flower",
    "crystal",
    "ember",
    "frost",
    "cloud",
    "stone",
];

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn word(&mut self) -> &'static str {
        WORDS[(self.next() as usize) % WORDS.len()]
    }
}

fn camel(word: &str) -> String {
    let mut characters = word.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

fn scale_items() -> usize {
    std::env::var("PICTO_FTS_SCALE_ITEMS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_ITEMS)
}

fn synthetic(index: usize, rng: &mut Lcg) -> PreparedImport {
    let name = match index % 3 {
        0 => format!("{}-{}_{}-{index}", rng.word(), rng.word(), rng.word()),
        1 => format!("{}{}Dump{index}", camel(rng.word()), camel(rng.word())),
        _ => format!("IMG_{}_{}", 1000 + (rng.next() % 9000), rng.word()),
    };
    let tag_count = 5 + (rng.next() as usize % 6);
    let source_text = (0..tag_count)
        .map(|_| rng.word())
        .collect::<Vec<_>>()
        .join(" ");
    PreparedImport {
        stable_key: format!("scale-{index}"),
        media_name: format!("{name}.png"),
        file_path: format!("/tmp/scale/{name}.png"),
        facts: ImmutableMediaFacts {
            mime: "image/png".into(),
            size_bytes: 1024,
            width: Some(800),
            height: Some(600),
            duration_ms: None,
            frame_count: Some(1),
            content_hash: format!("hash-scale-{index}"),
            perceptual_hash: None,
            palette: vec![LabColor {
                l: 50.0,
                a: 10.0,
                b: -5.0,
                weight: 1.0,
            }],
        },
        lifecycle: Lifecycle::Active,
        rating: Rating::Unrated,
        notes: index
            .is_multiple_of(10_000)
            .then(|| format!("release-marker-{index}")),
        tags: Vec::new(),
        folders: Vec::new(),
        source_urls: vec![format!("https://{}.example/{name}", rng.word())],
        source_identity: Some(picto_library::SourceIdentity {
            source_key: "scale-fixture".into(),
            source_item_key: format!("scale-{index}"),
            source_text: Some(source_text),
        }),
        imported_at_ms: 1_700_000_000_000 + index as i64,
        captured_at_ms: None,
    }
}

#[test]
#[ignore = "scale benchmark; run with --release -- --ignored --nocapture"]
fn text_search_stays_fast_at_scale() {
    let items = scale_items();
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("library.sqlite");
    let library = Library::create(&path).unwrap();
    let mut rng = Lcg(0x5eed);

    let ingest_started = Instant::now();
    for batch_start in (0..items).step_by(BATCH) {
        let inputs = (batch_start..(batch_start + BATCH).min(items))
            .map(|index| synthetic(index, &mut rng))
            .collect::<Vec<_>>();
        library.ingest_batch(&inputs).unwrap();
    }
    println!("ingest {items} items: {:?}", ingest_started.elapsed());

    let settle_started = Instant::now();
    let mut settled = 0usize;
    while library.settle_fts(SETTLE_BATCH).unwrap().is_some() {
        settled += SETTLE_BATCH;
    }
    let settle_elapsed = settle_started.elapsed();
    println!(
        "settle fts (~{settled} slots): {settle_elapsed:?} ({:.0} items/s)",
        items as f64 / settle_elapsed.as_secs_f64()
    );

    let checkpoint_started = Instant::now();
    library.database().checkpoint_wal().unwrap();
    println!("wal checkpoint: {:?}", checkpoint_started.elapsed());

    let total = |query: &str| {
        library
            .query(
                &RootQuery {
                    scope: ItemScope::All,
                    view: ViewQuerySpec {
                        filter: FilterExpr::Clause(FilterClause::Text {
                            field: TextField::Global,
                            query: query.into(),
                        }),
                        sort: ItemSort::default(),
                    },
                },
                &PageRequest::default(),
            )
            .unwrap()
            .total
    };

    for query in [
        "a",
        "ne",
        "new",
        "newground",
        "ewground",
        "sunset",
        "release-marker",
        "crystal frost ember",
        "xyzzynope",
    ] {
        let mut best = std::time::Duration::MAX;
        let mut matches = 0;
        for _ in 0..5 {
            let started = Instant::now();
            matches = total(query);
            best = best.min(started.elapsed());
        }
        println!("query {query:?}: {matches} matches, best of 5: {best:?}");
    }

    // Walk a filtered view page by page through the cursor, the way the grid
    // fetches while scrolling. Every page re-enters `query`, so this measures
    // whether the text match is recomputed per page.
    for (query, limit) in [("release-marker", 50), ("sunset", 200)] {
        let root_query = RootQuery {
            scope: ItemScope::All,
            view: ViewQuerySpec {
                filter: FilterExpr::Clause(FilterClause::Text {
                    field: TextField::Global,
                    query: query.into(),
                }),
                sort: ItemSort::default(),
            },
        };
        let started = Instant::now();
        let mut cursor = None;
        let mut pages = 0usize;
        let mut fetched = 0usize;
        loop {
            let page = library
                .query(&root_query, &PageRequest { limit, cursor })
                .unwrap();
            pages += 1;
            fetched += page.items.len();
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        let elapsed = started.elapsed();
        println!(
            "walk {query:?}: {fetched} items over {pages} pages of {limit}: {elapsed:?} ({:?}/page)",
            elapsed / pages as u32
        );
    }

    // Isolate the text-match cost from paging and scheduling overhead.
    library
        .database()
        .read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                for query in ["newground", "xyzzynope", "crystal frost ember"] {
                    let mut best = std::time::Duration::MAX;
                    let mut matched = 0;
                    for _ in 0..5 {
                        let started = Instant::now();
                        matched = picto_library::fts::search(connection, TextField::Global, query)
                            .unwrap()
                            .len();
                        best = best.min(started.elapsed());
                    }
                    println!("raw fts::search {query:?}: {matched} matches, best of 5: {best:?}");
                }
                Ok(())
            },
        )
        .unwrap();

    let bytes = std::fs::metadata(&path).unwrap().len();
    println!("database size: {:.1} MiB", bytes as f64 / (1024.0 * 1024.0));
    println!("database path: {}", path.display());
    // Hold the temp dir open long enough to inspect table sizes externally if
    // needed, then let it clean up.
}
