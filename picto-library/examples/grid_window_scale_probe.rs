//! Window-query benchmark, including a complete million-item traversal.
#[allow(dead_code)]
#[path = "grid_query_scale_probe.rs"]
mod fixture;
use picto_library::query::{ItemScope, PageRequest, RootQuery, WindowRequest};
use picto_library::{Library, SmartFolderInput};
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Instant;

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
fn stats(values: &mut [f64]) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    json!({"median_ms":values[values.len()/2], "p95_ms":values[(values.len()*95).div_ceil(100)-1], "max_ms":values[values.len()-1]})
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn stage_delta(
    before: &picto_library::query::QueryStatistics,
    after: &picto_library::query::QueryStatistics,
) -> serde_json::Value {
    json!({
        "match_builds":after.match_builds-before.match_builds,
        "match_ms":(after.match_us-before.match_us) as f64 / 1000.0,
        "aggregate_ms":(after.aggregate_us-before.aggregate_us) as f64 / 1000.0,
        "order_builds":after.order_builds-before.order_builds,
        "order_ms":(after.order_us-before.order_us) as f64 / 1000.0,
        "summary_ms":(after.summary_us-before.summary_us) as f64 / 1000.0,
        "summary_items":after.summary_items-before.summary_items,
    })
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let supplied_path = std::env::args().nth(1).map(std::path::PathBuf::from);
    let directory = supplied_path
        .is_none()
        .then(|| {
            tempfile::Builder::new()
                .prefix("picto-window-bench-")
                .tempdir()
        })
        .transpose()?;
    let path =
        supplied_path.unwrap_or_else(|| directory.as_ref().unwrap().path().join("library.sqlite"));
    if directory.is_some() {
        fixture::seed(&path, 1_000_000)?;
    }
    let library = Library::open(&path)?;
    let mut queries = fixture::queries();
    if directory.is_some() {
        for (label, query) in fixture::queries()
            .into_iter()
            .filter(|(label, _)| label.starts_with("tag_") || label.starts_with("text_"))
        {
            let (id, _) = library.create_smart_folder(SmartFolderInput {
                name: label.into(),
                parent_id: None,
                icon: None,
                color: None,
                notes: None,
                view: query.view,
            })?;
            queries.push((
                match label {
                    "tag_common" => "smart_common",
                    "tag_one_percent" => "smart_one_percent",
                    "tag_rare" => "smart_rare",
                    "text_rare" => "smart_text_rare",
                    _ => "smart_text_common",
                },
                RootQuery {
                    scope: ItemScope::SmartFolder {
                        smart_folder_id: id,
                    },
                    view: Default::default(),
                },
            ));
        }
    }
    for (label, query) in &queries {
        let stages_before = library.query_statistics();
        let started = Instant::now();
        let first = library.query_window(
            query,
            &WindowRequest {
                start: 0,
                limit: 500,
            },
        )?;
        let first_ms = ms(started);
        let started = Instant::now();
        let tail = library.query_window(
            query,
            &WindowRequest {
                start: usize::MAX,
                limit: 1500,
            },
        )?;
        let cold_jump_ms = ms(started);
        let mut samples = Vec::new();
        for step in 0..20 {
            let ordinal = first.page.total as usize * ((step * 37) % 100) / 100;
            let started = Instant::now();
            let result = library.query_window(
                query,
                &WindowRequest {
                    start: ordinal,
                    limit: 1500,
                },
            )?;
            samples.push(ms(started));
            assert!(result.page.items.len() <= 1500);
            assert_eq!(result.page.total, first.page.total);
        }
        let head = library.query_window(
            query,
            &WindowRequest {
                start: 0,
                limit: 1500,
            },
        )?;
        assert_eq!(head.page.items[..first.page.items.len()], first.page.items);
        println!(
            "CASE {}",
            json!({"case":label,"matches":first.page.total,"first_ms":first_ms,"cold_jump_ms":cold_jump_ms,"warm_jumps":stats(&mut samples),"tail_start":tail.start,"stages":stage_delta(&stages_before,&library.query_statistics())})
        );
    }
    // Reproduce a user repeatedly changing search/filter behavior and then
    // jumping to an arbitrary place in the resulting grid. The fixed seed
    // makes regressions comparable while still defeating sequential locality.
    let random_queries = queries
        .iter()
        .filter(|(label, _)| {
            matches!(
                *label,
                "all_imported"
                    | "random"
                    | "mime_png"
                    | "tag_common"
                    | "tag_one_percent"
                    | "tag_rare"
                    | "text_rare"
                    | "text_common"
            )
        })
        .collect::<Vec<_>>();
    let mut random_state = 0x5049_4354_4f5f_4752u64;
    let mut random_head_ms = Vec::new();
    let mut random_jump_ms = Vec::new();
    let mut random_by_query = BTreeMap::<&str, Vec<f64>>::new();
    for _ in 0..64 {
        let query_index = next_random(&mut random_state) as usize % random_queries.len();
        let (label, query) = random_queries[query_index];
        let started = Instant::now();
        let head = library.query_window(query, &WindowRequest { start: 0, limit: 4 })?;
        random_head_ms.push(ms(started));
        let total = head.page.total as usize;
        let start = if total == 0 {
            0
        } else {
            next_random(&mut random_state) as usize % total
        };
        let started = Instant::now();
        let window = library.query_window(query, &WindowRequest { start, limit: 1500 })?;
        let jump_ms = ms(started);
        random_jump_ms.push(jump_ms);
        random_by_query.entry(label).or_default().push(jump_ms);
        let verified_head = library.query_window(query, &WindowRequest { start: 0, limit: 4 })?;
        assert_eq!(window.page.total, head.page.total);
        assert_eq!(verified_head.page.items, head.page.items);
    }
    println!(
        "RANDOM_SEARCH {}",
        json!({
            "seed":"0x504943544f5f4752",
            "operations":64,
            "head_queries":stats(&mut random_head_ms),
            "arbitrary_jumps":stats(&mut random_jump_ms),
            "jump_ms_by_query":random_by_query.into_iter().map(|(label, mut samples)| {
                (label, stats(&mut samples))
            }).collect::<BTreeMap<_, _>>(),
            "query_head_verified":true,
        })
    );
    let query = &queries[0].1;
    let started = Instant::now();
    let first = library.query(
        query,
        &PageRequest {
            limit: 500,
            cursor: None,
        },
    )?;
    let mut expected: Vec<_> = first.items.into_iter().map(|item| item.root_id).collect();
    let mut cursor = first.next_cursor;
    while let Some(next) = cursor {
        let page = library.query(
            query,
            &PageRequest {
                limit: 500,
                cursor: Some(next),
            },
        )?;
        expected.extend(page.items.into_iter().map(|item| item.root_id));
        cursor = page.next_cursor;
    }
    let cursor_walk_ms = ms(started);
    let mut timings = Vec::new();
    let started = Instant::now();
    for start in (0..expected.len()).step_by(1000) {
        let began = Instant::now();
        let result = library.query_window(query, &WindowRequest { start, limit: 1500 })?;
        timings.push(ms(began));
        assert_eq!(
            result
                .page
                .items
                .iter()
                .map(|item| item.root_id)
                .collect::<Vec<_>>(),
            expected[result.start..(result.start + 1500).min(expected.len())]
        );
    }
    println!(
        "FULL_WALK {}",
        json!({"items":expected.len(),"windows":timings.len(),"cursor_walk_ms":cursor_walk_ms,"window_walk_ms":ms(started),"window_samples":stats(&mut timings),"max_hydrated_items":1500,"order_id_bytes":expected.len()*4})
    );
    // Mutations run only against the disposable fixture, never the supplied
    // renderable library used for manual testing.
    if directory.is_some() {
        use picto_library::selection::SelectionTarget;
        use picto_library::{ImmutableMediaFacts, Lifecycle, PreparedImport, Rating};
        let all = &queries[0].1;
        let head = library.query_window(
            all,
            &WindowRequest {
                start: 0,
                limit: 1500,
            },
        )?;
        let target = SelectionTarget::Explicit {
            root_ids: vec![head.page.items[0].root_id],
        };
        let baseline = library.query_statistics();
        let mut samples = Vec::new();
        for step in 0..32 {
            library.set_rating(
                &target,
                if step % 2 == 0 {
                    Rating::Five
                } else {
                    Rating::Unrated
                },
            )?;
            let started = Instant::now();
            let result = library.query_window(
                all,
                &WindowRequest {
                    start: next_random(&mut random_state) as usize % head.page.total as usize,
                    limit: 1500,
                },
            )?;
            samples.push(ms(started));
            assert_eq!(result.page.total, head.page.total);
            assert_eq!(result.page.media_count, head.page.media_count);
        }
        let after = library.query_statistics();
        assert_eq!(after.order_builds, baseline.order_builds);
        assert_eq!(after.match_builds, baseline.match_builds);
        println!(
            "RATING_DURING_JUMPS {}",
            json!({"windows":stats(&mut samples),"stages":stage_delta(&baseline,&after)})
        );
        let mut samples = Vec::new();
        for step in 0..8 {
            let key = format!("window-import-{step}");
            library.ingest(&PreparedImport {
                stable_key: key.clone(),
                media_name: key.clone(),
                file_path: format!("/synthetic/{key}.png"),
                facts: ImmutableMediaFacts {
                    mime: "image/png".into(),
                    size_bytes: 1024,
                    width: Some(640),
                    height: Some(480),
                    duration_ms: None,
                    frame_count: Some(1),
                    content_hash: key,
                    perceptual_hash: None,
                    palette: vec![],
                },
                lifecycle: Lifecycle::Active,
                rating: Rating::Unrated,
                notes: None,
                tags: vec!["common".into()],
                folders: vec![],
                source_urls: vec![],
                source_identity: None,
                imported_at_ms: 1_900_000_000_000 + step,
                captured_at_ms: None,
            })?;
            let started = Instant::now();
            let result = library.query_window(
                all,
                &WindowRequest {
                    start: usize::MAX,
                    limit: 1500,
                },
            )?;
            samples.push(ms(started));
            assert_eq!(result.page.total, head.page.total + step as u64 + 1);
            // Smart folders use the same canonical match set after each import.
            for (label, query) in queries
                .iter()
                .filter(|(label, _)| label.starts_with("smart_"))
            {
                let direct_label = match *label {
                    "smart_common" => "tag_common",
                    "smart_one_percent" => "tag_one_percent",
                    "smart_rare" => "tag_rare",
                    "smart_text_rare" => "text_rare",
                    _ => "text_common",
                };
                let direct = &queries
                    .iter()
                    .find(|(label, _)| *label == direct_label)
                    .unwrap()
                    .1;
                let actual = library.query_window(
                    query,
                    &WindowRequest {
                        start: usize::MAX,
                        limit: 4,
                    },
                )?;
                let expected = library.query_window(
                    direct,
                    &WindowRequest {
                        start: usize::MAX,
                        limit: 4,
                    },
                )?;
                assert_eq!(actual.page.total, expected.page.total);
                assert_eq!(actual.page.items, expected.page.items);
            }
        }
        println!(
            "IMPORT_DURING_JUMPS {}",
            json!({"windows":stats(&mut samples),"smart_folder_mismatches":0})
        );
    }
    drop(library);
    if let Some(directory) = directory {
        directory.close()?;
    }
    Ok(())
}
