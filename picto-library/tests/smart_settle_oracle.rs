//! Smart folders are settled incrementally: each edit re-evaluates only the
//! folders whose rules can observe it, over only the roots it touched. This is
//! the one test guarding that optimization. After every step of a long random
//! edit sequence, each folder's stored result must equal a fresh evaluation of
//! its rules over the whole library. Any missed dependency shows up as a
//! mismatch, whatever mechanism produced it.

use picto_library::predicate::{
    FilterClause, FilterExpr, ItemSort, SetMatchMode, TextField, ViewQuerySpec,
};
use picto_library::query::{matching_roots, ItemScope, MediaMatches, RootQuery};
use picto_library::selection::SelectionTarget;
use picto_library::{
    DuplicateResolutionChoice, ImmutableMediaFacts, Library, Lifecycle, PreparedImport, Rating, RootId, SmartFolderId,
    SmartFolderInput, TagId,
};

const ROOTS: usize = 240;
const STEPS: usize = 600;

/// Small deterministic generator; no extra dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
    fn subset(&mut self, ids: &[RootId], max: usize) -> Vec<RootId> {
        let count = 1 + self.below(max);
        let mut picked = (0..count).map(|_| ids[self.below(ids.len())]).collect::<Vec<_>>();
        picked.sort_by_key(|id| id.0);
        picked.dedup();
        picked
    }
}

fn root(index: usize) -> PreparedImport {
    PreparedImport {
        stable_key: format!("oracle-{index}"),
        media_name: format!("item-{index:04}.png"),
        file_path: format!("/oracle/{index}.png"),
        facts: ImmutableMediaFacts {
            mime: if index.is_multiple_of(3) { "image/jpeg".into() } else { "image/png".into() },
            size_bytes: 1_000 + index as u64 * 37,
            width: Some(400 + (index % 7) as u32 * 100),
            height: Some(300 + (index % 5) as u32 * 100),
            duration_ms: None,
            frame_count: Some(1),
            content_hash: format!("oracle-hash-{index}"),
            perceptual_hash: None,
            palette: Vec::new(),
        },
        lifecycle: Lifecycle::Active,
        rating: Rating::Unrated,
        notes: index.is_multiple_of(4).then(|| format!("note {index}")),
        tags: (0..(index % 4)).map(|n| format!("t{}", (index + n) % 8)).collect(),
        folders: Vec::new(),
        source_urls: Vec::new(),
        source_identity: None,
        imported_at_ms: 1_700_000_000_000 + index as i64,
        captured_at_ms: None,
    }
}

fn clause(clause: FilterClause) -> FilterExpr {
    FilterExpr::Clause(clause)
}

fn view(filter: FilterExpr) -> ViewQuerySpec {
    ViewQuerySpec {
        filter,
        sort: ItemSort::default(),
    }
}

fn explicit(ids: Vec<RootId>) -> SelectionTarget {
    SelectionTarget::Explicit { root_ids: ids }
}

fn tag_id(library: &Library, name: &str) -> Option<TagId> {
    library.projections().snapshot().tag_ids_by_name.get(name).copied()
}

#[test]
fn incremental_smart_folder_results_always_match_a_fresh_evaluation() {
    let directory = tempfile::tempdir().unwrap();
    let library = Library::create(directory.path().join("oracle.sqlite")).unwrap();
    for start in (0..ROOTS).step_by(picto_library::ingest::MAX_INGEST_BATCH) {
        let end = (start + picto_library::ingest::MAX_INGEST_BATCH).min(ROOTS);
        library.ingest_batch(&(start..end).map(root).collect::<Vec<_>>()).unwrap();
    }
    library.settle_fts(usize::MAX).unwrap();
    let ids = library
        .projections()
        .snapshot()
        .active()
        .iter()
        .map(RootId)
        .collect::<Vec<_>>();
    let (folder_a, _) = library.create_folder("A", None).unwrap();
    let (folder_b, _) = library.create_folder("B", None).unwrap();
    // Items kept apart from the edits, each merged once into an edited item
    // (a duplicate resolved by keeping the edited item's file).
    let mut merge_pool = library
        .ingest_batch(&(ROOTS..ROOTS + 40).map(root).collect::<Vec<_>>())
        .unwrap()
        .into_iter()
        .map(|(root_id, _)| root_id)
        .collect::<Vec<_>>();
    library.add_to_folder(&explicit(merge_pool[..20].to_vec()), folder_a).unwrap();
    library.add_to_folder(&explicit(merge_pool[10..30].to_vec()), folder_b).unwrap();
    library.set_rating(&explicit(merge_pool[..15].to_vec()), Rating::Five).unwrap();
    let t = |name: &str| tag_id(&library, name).unwrap();
    let edits_start = 1_800_000_000_000_u64;

    // One folder per rule kind an edit can affect, plus a nested folder.
    let rules: Vec<(&str, Option<usize>, FilterExpr)> = vec![
        ("any", None, clause(FilterClause::Tags { tag_ids: vec![t("t1"), t("t2")], mode: SetMatchMode::Any })),
        ("all", None, clause(FilterClause::Tags { tag_ids: vec![t("t1"), t("t2")], mode: SetMatchMode::All })),
        ("exact", None, clause(FilterClause::Tags { tag_ids: vec![t("t3")], mode: SetMatchMode::Exact })),
        ("not", None, FilterExpr::Not(Box::new(clause(FilterClause::Tags { tag_ids: vec![t("t4")], mode: SetMatchMode::Any })))),
        ("rated", None, clause(FilterClause::Ratings { ratings: vec![Rating::Four, Rating::Five] })),
        ("folder", None, clause(FilterClause::Folders { folder_ids: vec![folder_a], mode: SetMatchMode::Any })),
        ("folder-exact", None, clause(FilterClause::Folders { folder_ids: vec![folder_b], mode: SetMatchMode::Exact })),
        ("edited", None, clause(FilterClause::ModifiedAt { minimum_ms: Some(edits_start), maximum_ms: None })),
        ("named", None, clause(FilterClause::Text { field: TextField::Name, query: "renamed".into() })),
        ("noted", None, clause(FilterClause::NotesPresent { present: true })),
        ("wide-jpeg", None, FilterExpr::All(vec![
            clause(FilterClause::Width { minimum: Some(700), maximum: None }),
            clause(FilterClause::Mime { values: vec!["image/jpeg".into()], families: Vec::new() }),
        ])),
        ("nested", Some(0), clause(FilterClause::Ratings { ratings: vec![Rating::Unrated] })),
    ];
    let mut folders: Vec<(SmartFolderId, FilterExpr)> = Vec::new();
    for (name, parent, filter) in &rules {
        let parent_id = parent.map(|index| folders[index].0);
        let effective = match parent {
            Some(index) => FilterExpr::All(vec![folders[*index].1.clone(), filter.clone()]),
            None => filter.clone(),
        };
        let (id, _) = library
            .create_smart_folder(SmartFolderInput {
                name: (*name).into(),
                parent_id,
                icon: None,
                color: None,
                notes: None,
                view: view(filter.clone()),
            })
            .unwrap();
        folders.push((id, effective));
    }

    let mut rng = Rng(0x5eed_1234_abcd_0001);
    let mut clock = edits_start as i64;
    for step in 0..STEPS {
        clock += 1_000;
        let pick = rng.subset(&ids, 30);
        let tag = format!("t{}", rng.below(8));
        match rng.below(12) {
            0 => drop(library.apply_tags(&explicit(pick), &[tag], true).unwrap()),
            1 => drop(library.apply_tags(&explicit(pick), &[tag], false).unwrap()),
            2 => drop(library.set_rating(&explicit(pick), Rating::ALL[rng.below(Rating::ALL.len())]).unwrap()),
            3 => {
                let notes = (rng.below(2) == 0).then(|| "edited note".to_string());
                drop(library.patch_metadata(&explicit(pick), None, Some(notes), None, clock).unwrap());
            }
            4 => {
                let renames = pick.iter().map(|id| (*id, format!("renamed {}", id.0))).collect::<Vec<_>>();
                drop(library.rename_roots(&renames, clock).unwrap());
            }
            5 => {
                let lifecycle = [Lifecycle::Active, Lifecycle::Trash, Lifecycle::Inbox][rng.below(3)];
                drop(library.set_lifecycle(&explicit(pick), lifecycle).unwrap());
            }
            6 => drop(library.add_to_folder(&explicit(pick), [folder_a, folder_b][rng.below(2)]).unwrap()),
            7 => drop(library.remove_from_folder(&explicit(pick), [folder_a, folder_b][rng.below(2)]).unwrap()),
            8 => {
                // Merge a tag no folder references into one that some do.
                let source = format!("t{}", 5 + rng.below(3));
                if let (Some(source), Some(destination)) = (tag_id(&library, &source), tag_id(&library, "t1")) {
                    drop(library.merge_tags(source, destination, clock).unwrap());
                }
            }
            9 => drop(library.undo().unwrap()),
            10 => {
                if let Some(loser) = merge_pool.pop() {
                    let survivor = ids[rng.below(ids.len())];
                    let file = |root| library.details(root).unwrap().media[0].file_id;
                    let (loser_file, survivor_file) = (file(loser), file(survivor));
                    library.record_duplicate_pair(loser_file, survivor_file, 0, clock).unwrap();
                    drop(
                        library
                            .resolve_duplicate(
                                loser_file,
                                survivor_file,
                                DuplicateResolutionChoice::KeepFile { winner_file_id: survivor_file },
                                clock,
                            )
                            .unwrap(),
                    );
                }
            }
            _ => drop(library.redo().unwrap()),
        }
        library.settle_fts(usize::MAX).unwrap();

        let snapshot = library.projections().snapshot();
        // The sidebar's counts go through the library's cached path.
        let counts = library
            .sidebar_counts()
            .unwrap()
            .smart_folders
            .into_iter()
            .map(|entry| (entry.smart_folder_id, entry.count))
            .collect::<std::collections::HashMap<_, _>>();
        for (index, (id, effective)) in folders.iter().enumerate() {
            let stored = matching_roots(
                &snapshot,
                &RootQuery { scope: ItemScope::SmartFolder { smart_folder_id: *id }, view: ViewQuerySpec::default() },
                &MediaMatches::default(),
            )
            .unwrap();
            let fresh = matching_roots(
                &snapshot,
                &RootQuery { scope: ItemScope::All, view: view(effective.clone()) },
                &MediaMatches::default(),
            )
            .unwrap();
            assert_eq!(
                stored, fresh,
                "smart folder '{}' diverged from its rules after step {step}",
                rules[index].0
            );
            assert_eq!(
                counts.get(id).copied(),
                Some(stored.len() as u64),
                "smart folder '{}' count diverged after step {step}",
                rules[index].0
            );
        }
    }
}
