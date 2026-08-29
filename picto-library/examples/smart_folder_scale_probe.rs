use std::time::{Duration, Instant};

use picto_library::predicate::{FilterClause, FilterExpr, ItemSort, SetMatchMode, ViewQuerySpec};
use picto_library::query::{ItemScope, RootQuery};
use picto_library::selection::SelectionTarget;
use picto_library::{
    ImmutableMediaFacts, Library, Lifecycle, PreparedImport, Rating, SmartFolderId,
    SmartFolderInput, TagId,
};

fn main() -> picto_library::Result<()> {
    let root_count = argument(1, 1_000_000);
    let folder_count = argument(2, 1_000);
    let unique_filters = std::env::args().nth(3).as_deref() == Some("unique");
    assert!(folder_count >= 50, "use at least 50 smart folders");
    let depth_counts = depth_distribution(folder_count);
    let depth_offsets = depth_counts
        .iter()
        .scan(0, |offset, count| {
            let current = *offset;
            *offset += count;
            Some(current)
        })
        .collect::<Vec<_>>();

    let directory = tempfile::tempdir().map_err(picto_library::LibraryError::Io)?;
    let library = Library::create(directory.path().join("smart-scale.sqlite"))?;
    let ingest_started = Instant::now();
    for start in (0..root_count).step_by(picto_library::ingest::MAX_INGEST_BATCH) {
        let end = (start + picto_library::ingest::MAX_INGEST_BATCH).min(root_count);
        let inputs = (start..end).map(prepared).collect::<Vec<_>>();
        library.ingest_batch(&inputs)?;
        if end.is_multiple_of(100_000) || end == root_count {
            println!(
                "fixture_roots={end} elapsed_ms={:.3}",
                millis(ingest_started.elapsed())
            );
        }
    }
    let tag_id = library.projections().snapshot().tag_ids_by_name["benchmark:all"];

    let creation_started = Instant::now();
    let mut levels = Vec::<Vec<SmartFolderId>>::with_capacity(depth_counts.len());
    for (depth, count) in depth_counts.iter().copied().enumerate() {
        let mut level = Vec::with_capacity(count);
        for index in 0..count {
            let definition_index = depth_offsets[depth] + index;
            let parent_id =
                (depth != 0).then(|| levels[depth - 1][index % levels[depth - 1].len()]);
            level.push(create(
                &library,
                definition_index,
                parent_id,
                tag_id,
                filter_variant(unique_filters, definition_index),
            )?);
        }
        levels.push(level);
    }
    let creation_elapsed = creation_started.elapsed();

    let depth_eight_samples = update_samples(
        &library,
        levels[7][0],
        Some(levels[6][0]),
        tag_id,
        filter_variant(unique_filters, depth_offsets[7]),
        20,
    )?;
    let depth_three_samples = update_samples(
        &library,
        levels[2][0],
        Some(levels[1][0]),
        tag_id,
        filter_variant(unique_filters, depth_offsets[2]),
        20,
    )?;
    let depth_one_samples = update_samples(
        &library,
        levels[0][0],
        None,
        tag_id,
        filter_variant(unique_filters, 0),
        10,
    )?;
    let all = SelectionTarget::Query {
        query: RootQuery {
            scope: ItemScope::All,
            view: ViewQuerySpec::default(),
        },
        excluded_root_ids: Vec::new(),
    };
    let reject_started = Instant::now();
    library.set_rating(&all, Rating::One)?;
    let reject_elapsed = reject_started.elapsed();
    let restore_started = Instant::now();
    library.set_rating(&all, Rating::Unrated)?;
    let restore_elapsed = restore_started.elapsed();

    let depth_one_subtree = subtree_size(&depth_counts, 0, 0);
    let depth_three_subtree = subtree_size(&depth_counts, 2, 0);
    let weighted_recomputed = depth_counts
        .iter()
        .enumerate()
        .map(|(depth, count)| {
            (0..*count)
                .map(|index| subtree_size(&depth_counts, depth, index))
                .sum::<usize>()
        })
        .sum::<usize>() as f64
        / folder_count as f64;

    println!("roots={root_count}");
    println!("smart_folders={folder_count}");
    println!(
        "hierarchy_depths={}",
        depth_counts
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join("/")
    );
    println!("rules_per_level=10");
    println!("unique_local_filters={unique_filters}");
    println!("max_effective_rules=80");
    println!(
        "smart_folder_create_total_ms={:.3}",
        millis(creation_elapsed)
    );
    println!(
        "smart_folder_create_average_ms={:.3}",
        millis(creation_elapsed) / folder_count as f64
    );
    print_samples("depth_eight", 1, depth_eight_samples);
    print_samples("depth_three", depth_three_subtree, depth_three_samples);
    print_samples("depth_one", depth_one_subtree, depth_one_samples);
    println!("weighted_average_recomputed_folders={weighted_recomputed:.3}");
    println!(
        "all_folders_reject_all_roots_ms={:.3}",
        millis(reject_elapsed)
    );
    println!(
        "all_folders_restore_all_roots_ms={:.3}",
        millis(restore_elapsed)
    );
    let projection_bytes = library.projections().snapshot().estimated_bytes();
    println!("projection_estimated_bytes={projection_bytes}");
    println!(
        "projection_estimated_mib={:.3}",
        projection_bytes as f64 / (1024.0 * 1024.0)
    );
    Ok(())
}

fn depth_distribution(folder_count: usize) -> [usize; 8] {
    let weights = [20, 380, 400, 120, 50, 20, 8, 2];
    let mut counts = weights.map(|weight| (folder_count * weight / 1_000).max(1));
    let assigned = counts.iter().sum::<usize>();
    if assigned <= folder_count {
        counts[2] += folder_count - assigned;
    } else {
        counts[2] -= assigned - folder_count;
    }
    counts
}

fn subtree_size(counts: &[usize; 8], depth: usize, index: usize) -> usize {
    if depth + 1 == counts.len() {
        return 1;
    }
    1 + (0..counts[depth + 1])
        .filter(|child| child % counts[depth] == index)
        .map(|child| subtree_size(counts, depth + 1, child))
        .sum::<usize>()
}

fn create(
    library: &Library,
    index: usize,
    parent_id: Option<SmartFolderId>,
    tag_id: TagId,
    variant: u64,
) -> picto_library::Result<SmartFolderId> {
    library
        .create_smart_folder(input(index, parent_id, tag_id, variant))
        .map(|(id, _)| id)
}

fn update_samples(
    library: &Library,
    smart_folder_id: SmartFolderId,
    parent_id: Option<SmartFolderId>,
    tag_id: TagId,
    base_variant: u64,
    count: usize,
) -> picto_library::Result<Vec<Duration>> {
    (0..count)
        .map(|index| {
            let started = Instant::now();
            library.update_smart_folder(
                smart_folder_id,
                input(
                    smart_folder_id.0 as usize,
                    parent_id,
                    tag_id,
                    base_variant + (index % 2) as u64,
                ),
            )?;
            Ok(started.elapsed())
        })
        .collect()
}

fn filter_variant(unique_filters: bool, index: usize) -> u64 {
    unique_filters.then_some(index as u64).unwrap_or(0)
}

fn input(
    index: usize,
    parent_id: Option<SmartFolderId>,
    tag_id: TagId,
    variant: u64,
) -> SmartFolderInput {
    let minimum_size = 512_000 - variant;
    SmartFolderInput {
        name: format!("Smart {index}"),
        parent_id,
        icon: None,
        color: None,
        notes: None,
        view: ViewQuerySpec {
            filter: FilterExpr::All(vec![
                FilterExpr::Clause(FilterClause::Tags {
                    tag_ids: vec![tag_id],
                    mode: SetMatchMode::All,
                }),
                FilterExpr::Clause(FilterClause::Ratings {
                    ratings: vec![Rating::Unrated],
                }),
                FilterExpr::Clause(FilterClause::Mime {
                    values: vec!["image/png".into()],
                    families: Vec::new(),
                }),
                FilterExpr::Clause(FilterClause::ImportedAt {
                    minimum_ms: Some(1_700_000_000_000),
                    maximum_ms: None,
                }),
                FilterExpr::Clause(FilterClause::ModifiedAt {
                    minimum_ms: Some(1_700_000_000_000),
                    maximum_ms: None,
                }),
                FilterExpr::Clause(FilterClause::CapturedAt {
                    minimum_ms: Some(1_600_000_000_000),
                    maximum_ms: None,
                }),
                FilterExpr::Clause(FilterClause::Width {
                    minimum: Some(1_024),
                    maximum: None,
                }),
                FilterExpr::Clause(FilterClause::Height {
                    minimum: Some(768),
                    maximum: None,
                }),
                FilterExpr::Clause(FilterClause::TotalSize {
                    minimum_bytes: Some(minimum_size),
                    maximum_bytes: None,
                }),
                FilterExpr::Clause(FilterClause::SourceUrlsPresent { present: true }),
            ]),
            sort: ItemSort::default(),
        },
    }
}

fn prepared(index: usize) -> PreparedImport {
    PreparedImport {
        stable_key: format!("smart-root-{index}"),
        media_name: format!("image-{index:08}.png"),
        file_path: format!("/fixture/{index}.png"),
        facts: ImmutableMediaFacts {
            mime: "image/png".into(),
            size_bytes: 512_000 + (index % 4_096) as u64,
            width: Some(1_024 + (index % 1_024) as u32),
            height: Some(768 + (index % 768) as u32),
            duration_ms: None,
            frame_count: Some(1),
            content_hash: format!("smart-fixture-hash-{index}"),
            perceptual_hash: None,
            palette: Vec::new(),
        },
        lifecycle: Lifecycle::Active,
        rating: Rating::Unrated,
        notes: None,
        tags: vec!["benchmark:all".into()],
        folders: Vec::new(),
        source_urls: vec![format!("https://fixture.invalid/{index}")],
        source_identity: None,
        imported_at_ms: 1_700_000_000_000 + index as i64,
        captured_at_ms: Some(1_600_000_000_000 + index as i64),
    }
}

fn print_samples(label: &str, recomputed_folders: usize, mut samples: Vec<Duration>) {
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let p95 = samples[(samples.len() - 1) * 95 / 100];
    println!("{label}_subtree_folders={recomputed_folders}");
    println!("{label}_edit_p50_ms={:.3}", millis(median));
    println!("{label}_edit_p95_ms={:.3}", millis(p95));
    println!(
        "{label}_per_recomputed_folder_p50_ms={:.3}",
        millis(median) / recomputed_folders as f64
    );
}

fn argument(index: usize, default: usize) -> usize {
    std::env::args()
        .nth(index)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn millis(value: Duration) -> f64 {
    value.as_secs_f64() * 1_000.0
}
