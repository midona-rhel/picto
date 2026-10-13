use picto_library::{database::WorkPriority, Library};
use rusqlite::StatementStatus;

use super::{bench, R};

pub(super) fn run(library: &Library, roots: usize, samples: usize) -> R<()> {
    let posts = (roots / 100).max(1);
    library.auxiliary_write(WorkPriority::CanonicalIngest, ["subscriptions".into()], [], |tx, _| {
        tx.execute_batch(
            "INSERT INTO subscription(subscription_id, subscription_key, name, created_at)
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
            "INSERT INTO source_post(source_post_id, site_id, post_key, created_at, updated_at)
             SELECT root_id, 'test', CAST(root_id AS TEXT), 'now', 'now' FROM library_root LIMIT ?1",
            [posts as i64],
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
        library.auxiliary_write(
            WorkPriority::CanonicalIngest,
            ["subscriptions".into()],
            [],
            |tx, _| {
                tx.execute(
                "INSERT INTO source_post_attempt(run_query_id, source_post_id, state, started_at)
                 SELECT q.run_query_id, p.source_post_id,
                        CASE WHEN q.run_query_id = 1 THEN 'added'
                             WHEN q.run_query_id % 2 = 0 THEN 'failed' ELSE 'skipped' END, 'now'
                 FROM subscription_run_query q CROSS JOIN source_post p
                 WHERE q.run_query_id BETWEEN ?1 AND ?2", [start as i64, end as i64],
            )?;
                tx.execute(
                    "INSERT INTO source_file_attempt(attempt_id, source_item_id, state)
                 SELECT attempt_id, source_post_id,
                        CASE WHEN state = 'added' THEN 'retained'
                             WHEN state = 'failed' THEN 'failed' ELSE 'duplicate' END
                 FROM source_post_attempt WHERE run_query_id BETWEEN ?1 AND ?2",
                    [start as i64, end as i64],
                )?;
                Ok(())
            },
        )?;
        let size = format!(
            "{roots} roots; {} tagged; {} historical attempts; {posts} retained",
            roots / 5,
            posts * end
        );
        for (label, sql, count_column) in [
            (
                "subscription list",
                include_str!("../../../core/src/subscription_views.sql"),
                9,
            ),
            (
                "subscription queries",
                include_str!("../../../core/src/subscription_query_views.sql"),
                16,
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
                    library.auxiliary_read(WorkPriority::VisibleRead, |connection| {
                        let mut statement = connection.prepare(sql)?;
                        let count: i64 = statement.query_row([], |row| row.get(count_column))?;
                        assert_eq!(count, posts as i64);
                        println!(
                            "STEPS|{label}|{}",
                            statement.get_status(StatementStatus::VmStep)
                        );
                        Ok(())
                    })?;
                    Ok(())
                },
                None::<fn() -> R<()>>,
            )?;
        }
    }
    Ok(())
}
