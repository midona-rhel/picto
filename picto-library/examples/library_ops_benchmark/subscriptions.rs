use std::time::Instant;

use picto_library::{database::WorkPriority, Library};
use rusqlite::StatementStatus;

use super::{bench, R};

pub(super) fn run(mut library: Library, roots: usize, samples: usize) -> R<()> {
    let batch = (roots / 100).max(1);
    let path = library.database().path().to_owned();
    for duplicates in [false, true] {
        let posts = if duplicates { batch } else { batch * 100 };
        library.auxiliary_write(WorkPriority::CanonicalIngest, ["subscriptions".into()], [], |tx, _| {
            tx.execute_batch(
                "DELETE FROM subscription; DELETE FROM source_post;
                 INSERT INTO subscription(subscription_id, subscription_key, name, created_at)
                 VALUES (1, 'benchmark', 'Benchmark', 'now');
                 INSERT INTO subscription_query(query_id, query_key, subscription_id, site_id, domain_key, query_kind, query_text)
                 VALUES (1, 'benchmark', 1, 'test', 'test', 'tag', 'benchmark');
                 WITH RECURSIVE n(id) AS (VALUES(1) UNION ALL SELECT id + 1 FROM n WHERE id < 101)
                 INSERT INTO subscription_run(run_id, subscription_id, requested_by, status, created_at)
                 SELECT id, 1, 'user', CASE WHEN id = 101 THEN 'running' ELSE 'succeeded' END, 'now' FROM n;
                 INSERT INTO subscription_run_query(run_query_id, run_id, query_id, status, available_at)
                 SELECT run_id, run_id, 1, status, 'now' FROM subscription_run;",
            )?;
            tx.execute(
                "WITH RECURSIVE n(id) AS (VALUES(1) UNION ALL SELECT id + 1 FROM n WHERE id < ?1)
                 INSERT INTO source_post(source_post_id, site_id, post_key, created_at, updated_at)
                 SELECT id, 'test', CAST(id AS TEXT), 'now', 'now' FROM n", [posts as i64],
            )?;
            tx.execute_batch(
                "INSERT INTO source_item(source_item_id, source_post_id, item_key, position, state, created_at, updated_at)
                 SELECT source_post_id, source_post_id, 'file', 0, 'failed', 'now', 'now' FROM source_post;
                 INSERT INTO source_post_attempt(run_query_id, source_post_id, state, started_at)
                 SELECT 101, source_post_id, 'skipped', 'now' FROM source_post LIMIT 100;",
            )?;
            Ok(())
        })?;
        for (start, end) in [(1, 10), (11, 100)] {
            let started = Instant::now();
            library.auxiliary_write(WorkPriority::CanonicalIngest, ["subscriptions".into()], [], |tx, _| {
                tx.execute(
                    "INSERT INTO source_post_attempt(run_query_id, source_post_id, state, started_at)
                     SELECT q.run_query_id, p.source_post_id, 'added', 'now'
                     FROM subscription_run_query q JOIN source_post p
                       ON (?4 OR (p.source_post_id - 1) / ?3 + 1 = q.run_query_id)
                     WHERE q.run_query_id BETWEEN ?1 AND ?2",
                    rusqlite::params![start, end, batch as i64, duplicates],
                )?;
                tx.execute(
                    "INSERT INTO source_file_attempt(attempt_id, source_item_id, state)
                     SELECT attempt_id, source_post_id, 'retained' FROM source_post_attempt
                     WHERE run_query_id BETWEEN ?1 AND ?2", [start, end],
                )?;
                Ok(())
            })?;
            let expected = if duplicates {
                batch
            } else {
                batch * end as usize
            };
            let size = format!("{roots} roots; {} tagged; {} retained; {expected} distinct; redownloads={duplicates}", roots / 5, batch * end as usize);
            println!(
                "FIXTURE|{size}|append_ms={:.1}",
                started.elapsed().as_secs_f64() * 1000.0
            );
            library.auxiliary_write(WorkPriority::Maintenance, [], [], |tx, _| {
                let pages: i64 = tx.query_row("PRAGMA temp.page_count", [], |row| row.get(0))?;
                let bytes: i64 = tx.query_row("PRAGMA temp.page_size", [], |row| row.get(0))?;
                println!("MEMORY|{size}|temp_index_bytes={}", pages * bytes);
                Ok(())
            })?;
            // Exercise the actual catalog SQL plus the same revision's resident
            // accessor, not an isolated cache hit or an excluded-outcome fixture.
            for (label, sql) in [
                (
                    "subscription list",
                    include_str!("../../../core/src/subscription_views.sql"),
                ),
                (
                    "subscription queries",
                    include_str!("../../../core/src/subscription_query_views.sql"),
                ),
            ] {
                library.auxiliary_read(WorkPriority::VisibleRead, |connection| {
                    let mut plan = connection.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
                    for row in plan.query_map([], |row| row.get::<_, String>(3))? {
                        println!("PLAN|{label}|{}", row?);
                    }
                    Ok(())
                })?;
                bench(
                    label,
                    &size,
                    samples,
                    || {
                        library.auxiliary_read_consistent(
                            WorkPriority::VisibleRead,
                            |connection, snapshot| {
                                let mut statement = connection.prepare(sql)?;
                                let mut rows = statement.query([])?;
                                while rows.next()?.is_some() {}
                                drop(rows);
                                assert_eq!(
                                    snapshot.subscription_counts.query(1).media,
                                    expected as i64
                                );
                                assert_eq!(
                                    snapshot.subscription_counts.query(1).posts,
                                    expected as i64
                                );
                                assert_eq!(snapshot.subscription_counts.posts(1), expected as i64);
                                println!(
                                    "STEPS|{label}|{}",
                                    statement.get_status(StatementStatus::VmStep)
                                );
                                Ok(())
                            },
                        )?;
                        Ok(())
                    },
                    Some(|| {
                        // A new publication must not defeat the bounded read path.
                        library.auxiliary_write(
                            WorkPriority::CanonicalIngest,
                            ["tasks".into()],
                            [],
                            |tx, _| {
                                tx.execute(
                                    "UPDATE subscription_run SET status='running' WHERE run_id=101",
                                    [],
                                )?;
                                Ok(())
                            },
                        )?;
                        Ok(())
                    }),
                )?;
            }
            bench(
                "retained outcome update and publication",
                &size,
                samples,
                || {
                    library.auxiliary_write(WorkPriority::CanonicalIngest, ["subscriptions".into()], [], |tx, _| {
                    // Remove/reinsert one retained contribution. Refcounts must
                    // preserve successful redownload duplicates without scans.
                    let mut statement = tx.prepare(
                        "UPDATE source_file_attempt SET state='retained' WHERE file_attempt_id=(SELECT MIN(file_attempt_id) FROM source_file_attempt)",
                    )?;
                    statement.execute([])?;
                    println!("STEPS|retained update|{}", statement.get_status(StatementStatus::VmStep));
                    Ok(())
                })?;
                    assert_eq!(
                        library
                            .projections()
                            .snapshot()
                            .subscription_counts
                            .query(1)
                            .media,
                        expected as i64
                    );
                    Ok(())
                },
                None::<fn() -> R<()>>,
            )?;
        }
        drop(library);
        let started = Instant::now();
        library = Library::open(&path)?;
        assert_eq!(
            library
                .projections()
                .snapshot()
                .subscription_counts
                .query(1)
                .media,
            posts as i64
        );
        println!("ROW|subscription reopen and full resident rebuild|{posts} distinct; redownloads={duplicates}|{:.1}|-|n=1", started.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(())
}
