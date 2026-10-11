//! Cost of library operations on a large synthetic library.
//!
//! `cargo run --release -p picto_library --example library_ops_benchmark -- [roots] [runs] [smart_folders]`
//! (defaults: 1,000,000 roots, 5 runs, 100 smart folders; up to 1,000 smart folders).
//! Prints `ROW|operation|size|median ms|p90 ms|samples`. Local measurement only:
//! never commit its output.
use std::time::Instant;

use picto_library::predicate::{
    FilterClause, FilterExpr, ItemSort, SetMatchMode, TextField, ViewQuerySpec,
};
use picto_library::query::{ItemScope, RootQuery, WindowPosition, WindowRequest};
use picto_library::selection::{RangeSpan, SelectionTarget};
use picto_library::{
    LabColor,
    FolderId, GroupRequest, ImmutableMediaFacts, Library, Lifecycle, PreparedImport, Rating,
    RootId, SmartFolderId, SmartFolderInput,
};

type R<T> = Result<T, Box<dyn std::error::Error>>;
const VOCAB: usize = 20_000;
const BASE_MS: u64 = 1_700_000_000_000;

fn main() -> R<()> {
    let roots = arg(1, 1_000_000);
    let runs = arg(2, 5);
    // Leave room for the folders the smart-folder benches create.
    let smart_total = arg(3, 100).clamp(10, picto_library::smart::MAX_SMART_FOLDERS - 20);
    let at = |n: usize| n * roots / 1_000_000;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("audit.sqlite");
    let mut library = Library::create(&path)?;
    let fixture_started = Instant::now();

    // --- fixture: ingest ---
    let t = Instant::now();
    let mut ids: Vec<RootId> = Vec::with_capacity(roots);
    for start in (0..roots).step_by(picto_library::ingest::MAX_INGEST_BATCH) {
        let end = (start + picto_library::ingest::MAX_INGEST_BATCH).min(roots);
        for (id, _) in library.ingest_batch(&(start..end).map(prepared).collect::<Vec<_>>())? {
            ids.push(id);
        }
    }
    println!("FIXTURE ingest_{roots}_s={:.1}", t.elapsed().as_secs_f64());

    // --- fixture: settle FTS so name text filters see every root ---
    let t = Instant::now();
    while library.settle_fts(20_000)?.is_some() {}
    println!("FIXTURE settle_fts_s={:.1}", t.elapsed().as_secs_f64());

    // --- fixture: folders ---
    let t = Instant::now();
    let mut rng = 0x5049_4354_4f5fu64;
    let mut memberships = 0usize;
    for i in 0..300 {
        let (folder, _) = library.create_folder(&format!("Folder {i}"), None)?;
        let size = 1_000 + (next(&mut rng) as usize % 49_001);
        let offset = next(&mut rng) as usize % (roots - size);
        library.add_to_folder(&explicit(&ids[offset..offset + size]), folder)?;
        memberships += size;
    }
    let (big_folder, _) = library.create_folder("Big folder", None)?;
    library.add_to_folder(&explicit(&ids[at(100_000)..at(300_000)]), big_folder)?;
    memberships += at(200_000);
    println!(
        "FIXTURE folders=301 memberships={memberships} s={:.1}",
        t.elapsed().as_secs_f64()
    );

    // --- fixture: smart folders, every rule kind, every tenth nested ---
    let t = Instant::now();
    let snap = library.projections().snapshot();
    let vocab_ids: Vec<_> = (0..61)
        .map(|i| snap.tag_ids_by_name[&format!("vocab:{}", i * 97)])
        .collect();
    drop(snap);
    let mut smart_ids: Vec<SmartFolderId> = Vec::new();
    for i in 0..smart_total {
        let tag = vocab_ids[i % 60];
        let other = vocab_ids[(i + 7) % 60];
        let filter = match i % 10 {
            0..=2 => tag_filter(tag),
            3 => FilterExpr::Clause(FilterClause::Tags { tag_ids: vec![tag, other], mode: SetMatchMode::All }),
            4 => FilterExpr::Not(Box::new(tag_filter(tag))),
            5 => FilterExpr::Clause(FilterClause::Ratings { ratings: vec![Rating::Five] }),
            6 => FilterExpr::Clause(FilterClause::Folders { folder_ids: vec![big_folder], mode: SetMatchMode::Any }),
            7 => FilterExpr::Clause(FilterClause::ModifiedAt { minimum_ms: Some(BASE_MS + 1), maximum_ms: None }),
            8 => name_filter(&format!("image-{:04}", i % 100)),
            _ => FilterExpr::All(vec![
                FilterExpr::Clause(FilterClause::Width { minimum: Some(800), maximum: None }),
                FilterExpr::Clause(FilterClause::Mime { values: vec!["image/png".into()], families: Vec::new() }),
            ]),
        };
        let mut input = smart_input(&format!("Smart {i}"), filter);
        if i % 10 == 9 {
            input.parent_id = smart_ids.last().copied();
        }
        smart_ids.push(library.create_smart_folder(input)?.0);
    }
    println!("FIXTURE smart_folders={smart_total} s={:.1}", t.elapsed().as_secs_f64());

    // --- fixture: collections of 10 ---
    let t = Instant::now();
    let collections = 20;
    for c in 0..collections {
        let start = at(900_000) + c * 10;
        library.organize_into_collection(&group(&ids[start..start + 10], c))?;
    }
    println!("FIXTURE collections={collections}x10 s={:.1}", t.elapsed().as_secs_f64());
    library.history().clear();
    println!("FIXTURE total_s={:.1}", fixture_started.elapsed().as_secs_f64());
    print_memory(&library, "after_fixture");

    // --- selections ---
    let range_query = |scope: ItemScope, lo: usize, n: usize| SelectionTarget::Query {
        query: RootQuery {
            scope,
            view: ViewQuerySpec {
                filter: FilterExpr::Clause(FilterClause::ImportedAt {
                    minimum_ms: Some(BASE_MS + lo as u64),
                    maximum_ms: Some(BASE_MS + (lo + n) as u64 - 1),
                }),
                sort: ItemSort::default(),
            },
        },
        excluded_root_ids: Vec::new(),
    };
    let sel_200k = range_query(ItemScope::All, 0, at(200_000));
    let sel_200k_trash = range_query(ItemScope::Trash, 0, at(200_000));
    println!(
        "SELECTION 200k_query_size={}",
        library.selection_summary(&sel_200k)?.selected_count
    );
    let one = explicit(&ids[at(500_000)..at(500_000) + 1]);

    // --- mutations ---
    let mut k = 0usize;
    bench("apply_tags add 200k", "200000", runs, || {
        k += 1;
        library.apply_tags(&sel_200k, &[format!("audit:a{k}")], true)?;
        Ok(())
    }, Some(|| Ok(())))?;
    // remove the tags just added, timed separately
    let mut kr = 0usize;
    bench("apply_tags remove 200k", "200000", runs, || {
        kr += 1;
        library.apply_tags(&sel_200k, &[format!("audit:a{kr}")], false)?;
        Ok(())
    }, None::<fn() -> R<()>>)?;
    bench("apply_tags add 1", "1", runs * 4, || {
        library.apply_tags(&one, &["audit:one".into()], true)?;
        Ok(())
    }, Some(|| { library.apply_tags(&one, &["audit:one".into()], false)?; Ok(()) }))?;
    bench("apply_tags remove 1", "1", runs * 4, || {
        library.apply_tags(&one, &["audit:one".into()], false)?;
        Ok(())
    }, Some(|| { library.apply_tags(&one, &["audit:one".into()], true)?; Ok(()) }))?;
    library.apply_tags(&one, &["audit:one".into()], false)?;

    bench_with_reset("set_rating Four 200k (undo between)", "200000", runs, &library,
        |l| { l.set_rating(&sel_200k, Rating::Four)?; Ok(()) },
        |l| { l.undo()?; Ok(()) })?;
    bench_with_reset("set_rating Four 1 (undo between)", "1", runs * 4, &library,
        |l| { l.set_rating(&one, Rating::Four)?; Ok(()) },
        |l| { l.undo()?; Ok(()) })?;

    let mut trash_ms = Vec::new();
    let mut restore_ms = Vec::new();
    for _ in 0..runs {
        let t = Instant::now();
        library.set_lifecycle(&sel_200k, Lifecycle::Trash)?;
        trash_ms.push(ms(t));
        let t = Instant::now();
        library.set_lifecycle(&sel_200k_trash, Lifecycle::Active)?;
        restore_ms.push(ms(t));
    }
    emit("set_lifecycle Trash 200k", "200000", &mut trash_ms);
    emit("set_lifecycle restore Trash->Active 200k", "200000", &mut restore_ms);

    // folder (200k members) operations; root 600_000 is outside it
    let outsider = explicit(&ids[at(600_000)..at(600_000) + 1]);
    bench("add_to_folder 1 -> 200k folder", "1", runs * 2, || {
        library.add_to_folder(&outsider, big_folder)?;
        Ok(())
    }, Some(|| { library.remove_from_folder(&outsider, big_folder)?; Ok(()) }))?;
    bench("remove_from_folder 1 <- 200k folder", "1", runs * 2, || {
        library.remove_from_folder(&outsider, big_folder)?;
        Ok(())
    }, Some(|| { library.add_to_folder(&outsider, big_folder)?; Ok(()) }))?;
    library.remove_from_folder(&outsider, big_folder)?;
    // move one item inside a loaded 1,500-item window (send the window with one item moved)
    let order = library.projections().snapshot().folder_orders[&big_folder].clone();
    let window: Vec<RootId> = order[1_000..2_500].to_vec();
    let mut flip = false;
    bench("reorder_folder_items move 1 within 1.5k window of 200k folder", "1 (1500 sent)", runs * 2, || {
        flip = !flip;
        let mut moved = window.clone();
        if flip { let x = moved.remove(0); moved.push(x); }
        library.reorder_folder_items(big_folder, &moved)?;
        Ok(())
    }, None::<fn() -> R<()>>)?;

    let mut n = 0;
    bench("rename_root 1", "1", runs * 4, || {
        n += 1;
        library.rename_root(ids[at(500_000)], &format!("renamed-{n}.png"), now())?;
        Ok(())
    }, None::<fn() -> R<()>>)?;
    bench("set_notes 1", "1", runs * 4, || {
        n += 1;
        library.set_notes(&one, Some(format!("note {n}")), now())?;
        Ok(())
    }, None::<fn() -> R<()>>)?;

    let mut created: Vec<SmartFolderId> = Vec::new();
    let mut sc = 0;
    bench("create_smart_folder (tag rule)", "-", runs * 2, || {
        sc += 1;
        created.push(create_smart(&library, &format!("New smart {sc}"), tag_filter(vocab_ids[60]))?);
        Ok(())
    }, None::<fn() -> R<()>>)?;
    for id in created.drain(..) {
        library.delete_smart_folder(id)?;
    }
    let edit_target = smart_ids[0];
    let mut e = 0;
    bench("update_smart_folder (change tag rule)", "-", runs * 2, || {
        e += 1;
        library.update_smart_folder(edit_target, smart_input("Smart tag 0", tag_filter(vocab_ids[if e % 2 == 1 { 60 } else { 0 }])))?;
        Ok(())
    }, None::<fn() -> R<()>>)?;

    let mut cg = 0usize;
    let mut last_collection = None;
    bench("organize_into_collection 10", "10", runs, || {
        let start = at(950_000) + cg * 10;
        cg += 1;
        let (cid, _) = library.organize_into_collection(&group(&ids[start..start + 10], 10_000 + cg))?;
        last_collection = Some(cid);
        Ok(())
    }, None::<fn() -> R<()>>)?;
    // ungroup the collections just created (each sample a fresh one)
    let mut fresh = Vec::new();
    for c in 0..runs {
        let start = at(960_000) + c * 10;
        fresh.push(library.organize_into_collection(&group(&ids[start..start + 10], 20_000 + c))?.0);
    }
    let mut fi = fresh.into_iter();
    bench("ungroup_collection 10", "10", runs, || {
        library.ungroup_collection(fi.next().unwrap(), now())?;
        Ok(())
    }, None::<fn() -> R<()>>)?;
    let _ = last_collection;

    // undo / redo of a 200k tag op
    let mut undo_ms = Vec::new();
    let mut redo_ms = Vec::new();
    for i in 0..runs {
        library.apply_tags(&sel_200k, &[format!("audit:u{i}")], true)?;
        let t = Instant::now();
        library.undo()?;
        undo_ms.push(ms(t));
        let t = Instant::now();
        library.redo()?;
        redo_ms.push(ms(t));
        library.apply_tags(&sel_200k, &[format!("audit:u{i}")], false)?;
    }
    emit("undo apply_tags add 200k", "200000", &mut undo_ms);
    emit("redo apply_tags add 200k", "200000", &mut redo_ms);
    println!("HISTORY entries={} bytes={}", library.history().state().entries, library.history().state().bytes);

    let mut next_index = roots;
    bench("ingest_batch MAX_INGEST_BATCH new roots", &picto_library::ingest::MAX_INGEST_BATCH.to_string(), runs * 2, || {
        let batch = (next_index..next_index + picto_library::ingest::MAX_INGEST_BATCH).map(prepared).collect::<Vec<_>>();
        next_index += picto_library::ingest::MAX_INGEST_BATCH;
        library.ingest_batch(&batch)?;
        Ok(())
    }, None::<fn() -> R<()>>)?;

    let mut del_ms = Vec::new();
    for i in 0..runs {
        let start = at(700_000) + i * 1_000;
        let target = explicit(&ids[start..start + 1_000]);
        library.set_lifecycle(&target, Lifecycle::Trash)?;
        let t = Instant::now();
        library.permanently_delete(&target, now())?;
        del_ms.push(ms(t));
    }
    emit("permanently_delete 1k (from Trash)", "1000", &mut del_ms);
    while library.settle_fts(20_000)?.is_some() {}

    // --- reads ---
    bench("sidebar_counts", "-", runs * 4, || { library.sidebar_counts()?; Ok(()) }, None::<fn() -> R<()>>)?;
    bench("counts", "-", runs * 4, || { library.counts()?; Ok(()) }, None::<fn() -> R<()>>)?;
    let tag_count = library.tags()?.len();
    bench("tags (full list)", &format!("{tag_count} tags"), runs * 4, || { library.tags()?; Ok(()) }, None::<fn() -> R<()>>)?;
    bench("tag_page search (12)", "-", runs * 4, || { library.tag_page(None, Some("vocab:12"), 0, 12)?; Ok(()) }, None::<fn() -> R<()>>)?;
    let some_ids = library.tags()?.into_iter().take(5).map(|tag| tag.tag_id).collect::<Vec<_>>();
    bench("tags_by_id (5)", "-", runs * 4, || { library.tags_by_id(&some_ids)?; Ok(()) }, None::<fn() -> R<()>>)?;
    bench("unused_tag_count", "-", runs * 4, || { library.unused_tag_count()?; Ok(()) }, None::<fn() -> R<()>>)?;
    bench("navigation (folders+smart)", &format!("301 folders, {smart_total} smart"), runs * 4, || { library.navigation()?; Ok(()) }, None::<fn() -> R<()>>)?;
    bench("details 1", "1", runs * 4, || { library.details(ids[at(500_000)])?; Ok(()) }, None::<fn() -> R<()>>)?;

    let rating_smart = smart_ids[5];
    let text_smart = smart_ids[8];
    let windows: Vec<(&str, RootQuery)> = vec![
        ("All imported desc", RootQuery { scope: ItemScope::All, view: ViewQuerySpec::default() }),
        ("SmartFolder (rating=5)", RootQuery { scope: ItemScope::SmartFolder { smart_folder_id: rating_smart }, view: ViewQuerySpec::default() }),
        ("SmartFolder (name text)", RootQuery { scope: ItemScope::SmartFolder { smart_folder_id: text_smart }, view: ViewQuerySpec::default() }),
        ("All by name", RootQuery { scope: ItemScope::All, view: ViewQuerySpec { sort: ItemSort { field: picto_library::predicate::SortField::Name, direction: picto_library::predicate::SortDirection::Ascending, random_seed: None }, ..Default::default() } }),
        ("Tag filter (vocab:4850)", RootQuery { scope: ItemScope::All, view: ViewQuerySpec { filter: tag_filter(vocab_ids[50]), sort: ItemSort::default() } }),
        ("Text filter name 'image-0012'", RootQuery { scope: ItemScope::All, view: ViewQuerySpec { filter: name_filter("image-0012"), sort: ItemSort::default() } }),
        ("Folder 200k (folder order)", RootQuery { scope: ItemScope::Folder { folder_id: big_folder }, view: ViewQuerySpec { sort: ItemSort { field: picto_library::predicate::SortField::FolderOrder, direction: picto_library::predicate::SortDirection::Ascending, random_seed: None }, ..Default::default() } }),
    ];
    let bump = explicit(&ids[1..2]);
    for (label, query) in &windows {
        let first = library.query_window(query, &WindowRequest::at(0, 500))?;
        let total = first.page.total as usize;
        // warm: same revision, cache hit
        bench(&format!("window first 500 warm: {label}"), &total.to_string(), runs * 4, || {
            library.query_window(query, &WindowRequest::at(0, 500))?; Ok(())
        }, None::<fn() -> R<()>>)?;
        // after an unrelated 1-root tag edit (new revision)
        let mut b = false;
        bench(&format!("window first 500 after 1-root edit: {label}"), &total.to_string(), runs * 2, || {
            library.query_window(query, &WindowRequest::at(0, 500))?; Ok(())
        }, Some(|| { b = !b; library.apply_tags(&bump, &["audit:bump".into()], b)?; Ok(()) }))?;
        let mut b2 = false;
        bench(&format!("window jump middle 1500 after 1-root edit: {label}"), &total.to_string(), runs * 2, || {
            library.query_window(query, &WindowRequest::at(total / 2, 1_500))?; Ok(())
        }, Some(|| { b2 = !b2; library.apply_tags(&bump, &["audit:bump".into()], b2)?; Ok(()) }))?;
        bench(&format!("window jump middle 1500 warm: {label}"), &total.to_string(), runs * 4, || {
            library.query_window(query, &WindowRequest::at(total / 2, 1_500))?; Ok(())
        }, None::<fn() -> R<()>>)?;
        // Back to the bottom of a view: the request the grid sends when it
        // restores a saved scroll fraction of 1.0, after an unrelated edit.
        let bottom = WindowRequest { position: WindowPosition::Fraction(1.0), limit: 1_500 };
        let landed = library.query_window(query, &bottom)?;
        let end = landed.start + landed.page.items.len();
        if end != landed.page.total as usize {
            return Err(format!("{label}: fraction 1.0 ended at {end} of {}", landed.page.total).into());
        }
        let mut b3 = false;
        bench(&format!("window restore bottom 1500 after 1-root edit: {label}"), &total.to_string(), runs * 2, || {
            library.query_window(query, &bottom)?; Ok(())
        }, Some(|| { b3 = !b3; library.apply_tags(&bump, &["audit:bump".into()], b3)?; Ok(()) }))?;
    }
    // --- search and filters: every run uses a fresh query, so no cache helps ---
    let all = |filter: FilterExpr| RootQuery { scope: ItemScope::All, view: ViewQuerySpec { filter, sort: ItemSort::default() } };
    let text = |q: String| FilterExpr::Clause(FilterClause::Text { field: TextField::Global, query: q });
    let clause = FilterExpr::Clause;
    let mut i = 0usize;
    let mut cold = |label: &str, make: &mut dyn FnMut(usize) -> RootQuery| -> R<()> {
        let mut samples = Vec::new();
        let mut total = 0;
        for _ in 0..runs * 2 {
            i += 1;
            let query = make(i);
            let t = Instant::now();
            total = library.query_window(&query, &WindowRequest::at(0, 500))?.page.total;
            samples.push(ms(t));
        }
        emit(label, &total.to_string(), &mut samples);
        Ok(())
    };
    cold("search text rare (1 hit)", &mut |i| all(text(format!("ge-00{:06}", 100_000 + i * 1_111))))?;
    cold("search text common (~1k hits)", &mut |i| all(text(format!("image-00{:03}", 100 + i))))?;
    cold("search text no match", &mut |i| all(text(format!("zq{i}xw"))))?;
    cold("search text in notes", &mut |i| all(text(format!("scene {}", 1_000 + i))))?;
    cold("search text + tag (narrow)", &mut |i| all(FilterExpr::All(vec![tag_filter(vocab_ids[i % 60]), text(format!("image-0{i}"))])))?;
    cold("search text + rating 5 (100k)", &mut |i| all(FilterExpr::All(vec![clause(FilterClause::Ratings { ratings: vec![Rating::Five] }), text(format!("image-00{:02}", i))])))?;
    cold("filter width range", &mut |i| all(clause(FilterClause::Width { minimum: Some(800 + i as u64), maximum: Some(2_000) })))?;
    cold("filter imported range 30%", &mut |i| all(clause(FilterClause::ImportedAt { minimum_ms: Some(BASE_MS + (i * 1_000) as u64), maximum_ms: Some(BASE_MS + at(300_000) as u64 + (i * 1_000) as u64) })))?;
    cold("filter captured range", &mut |i| all(clause(FilterClause::CapturedAt { minimum_ms: Some(BASE_MS - at(500_000) as u64 * 1_000 + i as u64), maximum_ms: Some(BASE_MS) })))?;
    cold("filter size range", &mut |i| all(clause(FilterClause::TotalSize { minimum_bytes: Some(1_000_000 + i as u64), maximum_bytes: Some(2_000_000) })))?;
    cold("filter color", &mut |i| all(clause(FilterClause::Color { color: LabColor { l: 50.0, a: 10.0 + i as f32, b: -20.0, weight: 1.0 }, delta_e: 40.0 })))?;
    cold("filter tags exact", &mut |i| all(clause(FilterClause::Tags { tag_ids: vec![vocab_ids[i % 60]], mode: SetMatchMode::Exact })))?;
    cold("filter notes present + urls absent", &mut |i| all(FilterExpr::All(vec![clause(FilterClause::NotesPresent { present: true }), clause(FilterClause::SourceUrlsPresent { present: i % 2 == 0 })])))?;
    cold("filter complex (any tags, not tag, rating, width, text)", &mut |i| all(FilterExpr::All(vec![
        FilterExpr::Any(vec![tag_filter(vocab_ids[i % 60]), tag_filter(vocab_ids[(i + 1) % 60]), clause(FilterClause::Ratings { ratings: vec![Rating::Five] })]),
        FilterExpr::Not(Box::new(tag_filter(vocab_ids[(i + 2) % 60]))),
        clause(FilterClause::Width { minimum: Some(800 + i as u64), maximum: None }),
        text(format!("image-0{}", i % 10)),
    ])))?;
    // What typing feels like: each keystroke of a growing query, end to end.
    let mut typed = Vec::new();
    for word in ["image-000", "scene 12", "example.test/post/77"] {
        let t = Instant::now();
        for end in 3..=word.chars().count() {
            let prefix: String = word.chars().take(end).collect();
            library.query_window(&all(text(prefix)), &WindowRequest::at(0, 500))?;
        }
        typed.push(ms(t));
    }
    emit("search typing a word (every keystroke, total)", "3 words", &mut typed);
    // Select everything a search found and move it to Trash, then back.
    let found = SelectionTarget::Query { query: all(FilterExpr::All(vec![clause(FilterClause::Ratings { ratings: vec![Rating::Five] }), text("image-00".into())])), excluded_root_ids: Vec::new() };
    let found_trash = SelectionTarget::Query { query: RootQuery { scope: ItemScope::Trash, view: ViewQuerySpec::default() }, excluded_root_ids: Vec::new() };
    let mut trash = Vec::new();
    let mut back = Vec::new();
    for _ in 0..runs {
        let t = Instant::now();
        library.set_lifecycle(&found, Lifecycle::Trash)?;
        trash.push(ms(t));
        let t = Instant::now();
        library.set_lifecycle(&found_trash, Lifecycle::Active)?;
        back.push(ms(t));
    }
    emit("trash select-all of a search (rating 5 + text)", "-", &mut trash);
    emit("restore all of Trash", "-", &mut back);
    bench("selection summary of a search select-all", "-", runs, || { library.selection_summary(&found)?; Ok(()) }, None::<fn() -> R<()>>)?;
    // Shift-click range selection across 50k items of the main view.
    let first_page = library.query_window(&all(FilterExpr::All(Vec::new())), &WindowRequest::at(at(100_000), 1))?;
    let far_page = library.query_window(&all(FilterExpr::All(Vec::new())), &WindowRequest::at(at(150_000), 1))?;
    let span = |positions: bool| RangeSpan {
        anchor_root_id: first_page.page.items[0].root_id,
        focus_root_id: far_page.page.items[0].root_id,
        anchor_index: positions.then_some(first_page.start as u64),
        focus_index: positions.then_some(far_page.start as u64),
    };
    let range_with = |positions: bool| SelectionTarget::Range {
        query: all(FilterExpr::All(Vec::new())),
        spans: vec![span(positions)],
        included_root_ids: Vec::new(),
        excluded_root_ids: Vec::new(),
    };
    let range = range_with(false);
    bench("selection summary of a 50k shift-click range", "50000", runs, || { library.selection_summary(&range)?; Ok(()) }, None::<fn() -> R<()>>)?;
    // The first range after an edit, by ids only and with the grid's positions.
    let positioned = range_with(true);
    let touch = SelectionTarget::Explicit { root_ids: vec![first_page.page.items[0].root_id] };
    for (label, target) in [("first 50k range after an edit (ids only)", &range), ("first 50k range after an edit (grid positions)", &positioned)] {
        let mut samples = Vec::new();
        for _ in 0..runs {
            library.set_lifecycle(&touch, Lifecycle::Trash)?;
            library.set_lifecycle(&touch, Lifecycle::Active)?;
            let t = Instant::now();
            library.selection_summary(target)?;
            samples.push(ms(t));
        }
        emit(label, "50000", &mut samples);
    }
    // Typing into the tag picker: one tag search per keystroke.
    let mut picked = Vec::new();
    for word in ["vocab:12", "vocab:4"] {
        let t = Instant::now();
        for end in 1..=word.len() {
            library.tag_page(None, Some(&word[..end]), 0, 12)?;
        }
        picked.push(ms(t));
    }
    emit("tag search typing a word (every keystroke, total)", "2 words", &mut picked);
    let mut s = 0usize;
    bench("create smart folder (text rule)", "-", runs, || {
        s += 1;
        let (id, _) = library.create_smart_folder(smart_input(&format!("Text {s}"), text(format!("image-0{s}"))))?;
        library.delete_smart_folder(id)?;
        Ok(())
    }, None::<fn() -> R<()>>)?;

    // The first window after an import: new roots change the date index the
    // default sort reads.
    let mut import_index = roots + 1_000_000;
    let newest = RootQuery { scope: ItemScope::All, view: ViewQuerySpec::default() };
    bench("window first 500 after a 48-item import: All imported desc", "-", runs * 2, || {
        library.query_window(&newest, &WindowRequest::at(0, 500))?; Ok(())
    }, Some(|| {
        let batch = (import_index..import_index + picto_library::ingest::MAX_INGEST_BATCH).map(prepared).collect::<Vec<_>>();
        import_index += picto_library::ingest::MAX_INGEST_BATCH;
        library.ingest_batch(&batch)?;
        Ok(())
    }))?;

    print_memory(&library, "before_reopen");

    // --- close / reopen ---
    let t = Instant::now();
    let checkpoint_bytes = library.write_projection_checkpoint()?;
    println!("ROW|write_projection_checkpoint|-|{:.1}|-|1 run; {checkpoint_bytes} bytes", ms(t));
    let mut open_ms = Vec::new();
    for _ in 0..3 {
        drop(library);
        let t = Instant::now();
        library = Library::open(&path)?;
        open_ms.push(ms(t));
    }
    emit("Library::open (fresh checkpoint)", &format!("{} roots", roots), &mut open_ms);
    let t = Instant::now();
    let first = library.query_window(&windows[0].1, &WindowRequest::at(0, 500))?;
    println!("ROW|first window after reopen (All)|{}|{:.1}|-|1 run, cold caches", first.page.total, ms(t));
    print_memory(&library, "after_reopen");
    Ok(())
}

fn bench(
    label: &str,
    size: &str,
    runs: usize,
    mut f: impl FnMut() -> R<()>,
    mut between: Option<impl FnMut() -> R<()>>,
) -> R<()> {
    let mut samples = Vec::new();
    for _ in 0..runs {
        if let Some(b) = between.as_mut() {
            b()?;
        }
        let t = Instant::now();
        f()?;
        samples.push(ms(t));
    }
    emit(label, size, &mut samples);
    Ok(())
}

fn bench_with_reset(
    label: &str,
    size: &str,
    runs: usize,
    library: &Library,
    f: impl Fn(&Library) -> R<()>,
    reset: impl Fn(&Library) -> R<()>,
) -> R<()> {
    let mut samples = Vec::new();
    for _ in 0..runs {
        let t = Instant::now();
        f(library)?;
        samples.push(ms(t));
        reset(library)?;
    }
    emit(label, size, &mut samples);
    Ok(())
}

fn emit(label: &str, size: &str, samples: &mut [f64]) {
    let raw = samples.iter().map(|v| format!("{v:.1}")).collect::<Vec<_>>().join(",");
    samples.sort_by(f64::total_cmp);
    let n = samples.len();
    let median = if n % 2 == 1 { samples[n / 2] } else { (samples[n / 2 - 1] + samples[n / 2]) / 2.0 };
    let p95 = if n >= 10 { format!("{:.1}", samples[((n * 95).div_ceil(100)) - 1]) } else { "-".into() };
    println!("ROW|{label}|{size}|{median:.1}|{p95}|n={n} max={:.1} [{raw}]", samples[n - 1]);
}

fn print_memory(library: &Library, label: &str) {
    let bytes = library.projections().snapshot().estimated_bytes();
    let rss = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS")).map(|l| l.to_string()))
        .unwrap_or_default();
    println!("MEMORY {label} projection_estimated_bytes={bytes} ({:.1} MiB) {rss}", bytes as f64 / 1048576.0);
}

fn explicit(ids: &[RootId]) -> SelectionTarget {
    SelectionTarget::Explicit { root_ids: ids.to_vec() }
}

fn group(ids: &[RootId], n: usize) -> GroupRequest {
    GroupRequest {
        target: explicit(ids),
        cover_root_id: ids[0],
        winning_collection_id: None,
        name: Some(format!("Collection {n}")),
        notes: None,
        modified_at_ms: now(),
    }
}

fn tag_filter(tag_id: picto_library::TagId) -> FilterExpr {
    FilterExpr::Clause(FilterClause::Tags { tag_ids: vec![tag_id], mode: SetMatchMode::All })
}

fn name_filter(q: &str) -> FilterExpr {
    FilterExpr::Clause(FilterClause::Text { field: TextField::Name, query: q.into() })
}

fn smart_input(name: &str, filter: FilterExpr) -> SmartFolderInput {
    SmartFolderInput {
        name: name.into(),
        parent_id: None,
        icon: None,
        color: None,
        notes: None,
        view: ViewQuerySpec { filter, sort: ItemSort::default() },
    }
}

fn create_smart(library: &Library, name: &str, filter: FilterExpr) -> R<SmartFolderId> {
    Ok(library.create_smart_folder(smart_input(name, filter))?.0)
}

fn prepared(index: usize) -> PreparedImport {
    PreparedImport {
        stable_key: format!("root-{index}"),
        media_name: format!("image-{index:08}.png"),
        file_path: format!("/fixture/{index}.png"),
        facts: ImmutableMediaFacts {
            mime: "image/png".into(),
            size_bytes: 512_000 + (index * 7_919 % 4_000_000) as u64,
            width: Some(400 + (index * 37 % 3_600) as u32),
            height: Some(300 + (index * 53 % 2_700) as u32),
            duration_ms: None,
            frame_count: Some(1),
            content_hash: format!("hash-{index}"),
            perceptual_hash: None,
            palette: vec![LabColor {
                l: (index * 13 % 100) as f32,
                a: (index * 29 % 160) as f32 - 80.0,
                b: (index * 41 % 160) as f32 - 80.0,
                weight: 1.0,
            }],
        },
        lifecycle: Lifecycle::Active,
        rating: if index.is_multiple_of(10) { Rating::Five } else { Rating::Unrated },
        notes: index.is_multiple_of(7).then(|| format!("note about scene {}", index % 5_000)),
        tags: vec![format!("vocab:{}", index % VOCAB), format!("vocab:{}", (index * 7 + 3) % VOCAB)],
        folders: Vec::new(),
        source_urls: if index.is_multiple_of(5) {
            vec![format!("https://example.test/post/{index}")]
        } else {
            Vec::new()
        },
        source_identity: None,
        imported_at_ms: BASE_MS as i64 + index as i64,
        captured_at_ms: index.is_multiple_of(2).then_some(BASE_MS as i64 - index as i64 * 1_000),
    }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn arg(i: usize, default: usize) -> usize {
    std::env::args().nth(i).and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[allow(dead_code)]
fn unused(_: FolderId) {}
