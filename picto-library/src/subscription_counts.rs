//! Disposable outcome index owned by the canonical writer. Unlike a revision
//! cache, new retained outcomes cost only indexed row updates, not a rescan of
//! that query's history. Nothing here is persisted or checkpointed.
use std::collections::HashMap;

use rusqlite::Connection;

use crate::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueryCounts {
    pub posts: i64,
    pub media: i64,
    pub successful_runs: i64,
}

#[derive(Debug, Default)]
pub struct SubscriptionCounts {
    queries: HashMap<i64, QueryCounts>,
    subscriptions: HashMap<i64, i64>,
}

impl SubscriptionCounts {
    pub fn query(&self, query_id: i64) -> QueryCounts {
        self.queries.get(&query_id).copied().unwrap_or_default()
    }

    pub fn posts(&self, subscription_id: i64) -> i64 {
        self.subscriptions
            .get(&subscription_id)
            .copied()
            .unwrap_or(0)
    }

    pub(crate) fn read(connection: &Connection) -> Result<Self> {
        let mut counts = Self::default();
        let mut statement = connection.prepare_cached(
            "SELECT scope, owner_id, kind, total FROM temp.subscription_outcome_total",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let owner = row.get(1)?;
            let total = row.get(3)?;
            if row.get::<_, i64>(0)? == 1 {
                counts.subscriptions.insert(owner, total);
            } else {
                let query = counts.queries.entry(owner).or_default();
                match row.get::<_, i64>(2)? {
                    0 => query.posts = total,
                    1 => query.media = total,
                    _ => query.successful_runs = total,
                }
            }
        }
        Ok(counts)
    }

    pub(crate) fn dirty(connection: &Connection) -> Result<bool> {
        Ok(connection.query_row(
            "SELECT dirty FROM temp.subscription_counts_dirty",
            [],
            |row| row.get(0),
        )?)
    }
}

pub(crate) fn initialize(connection: &Connection) -> Result<()> {
    crate::query_request::check()?;
    connection.progress_handler(10_000, Some(|| crate::query_request::check().is_err()))?;
    let result = initialize_inner(connection);
    connection.progress_handler(0, None::<fn() -> bool>)?;
    crate::query_request::check()?;
    result
}

fn initialize_inner(connection: &Connection) -> Result<()> {
    connection.execute_batch(include_str!("subscription_counts.sql"))?;
    // Refresh only the outcome whose canonical row changed. BEFORE DELETE
    // also handles FK cascades: attribution comes from the disposable index,
    // not from a parent row which SQLite may already have removed.
    for (table, id, kind, view, columns) in [
        (
            "source_post_attempt",
            "attempt_id",
            0,
            "subscription_post_outcome",
            "state, run_query_id, source_post_id, attempt_id",
        ),
        (
            "source_file_attempt",
            "file_attempt_id",
            1,
            "subscription_file_outcome",
            "state, attempt_id, source_item_id, file_attempt_id",
        ),
        (
            "subscription_run_query",
            "run_query_id",
            2,
            "subscription_run_outcome",
            "status, query_id, run_query_id",
        ),
    ] {
        let remove =
            format!("DELETE FROM subscription_outcome WHERE kind = {kind} AND id = OLD.{id};");
        let insert =
            format!("INSERT INTO subscription_outcome SELECT * FROM {view} WHERE id = NEW.{id};");
        connection.execute_batch(&format!(
            "CREATE TEMP TRIGGER counts_{table}_insert AFTER INSERT ON main.{table} BEGIN {insert} END;
             CREATE TEMP TRIGGER counts_{table}_delete BEFORE DELETE ON main.{table} BEGIN {remove} END;
             CREATE TEMP TRIGGER counts_{table}_update AFTER UPDATE OF {columns} ON main.{table}
             BEGIN {remove} {insert} END;"
        ))?;
    }
    // Identity reparenting is rare, but must invalidate the same index. These
    // indexed refreshes are limited to the moved query/run/post's outcomes.
    for (table, columns, predicate, when) in [
        ("subscription_query", "subscription_id", "query_id = NEW.query_id", "OLD.subscription_id IS NOT NEW.subscription_id"),
        ("subscription_run_query", "query_id", "id IN (SELECT attempt_id FROM main.source_post_attempt WHERE run_query_id = NEW.run_query_id)", "OLD.query_id IS NOT NEW.query_id"),
    ] {
        let (post_predicate, file_predicate) = if table == "subscription_query" {
            (predicate.to_string(), predicate.to_string())
        } else {
            (predicate.to_string(), "id IN (SELECT f.file_attempt_id FROM main.source_post_attempt a JOIN main.source_file_attempt f USING(attempt_id) WHERE a.run_query_id = NEW.run_query_id)".into())
        };
        connection.execute_batch(&format!(
            "CREATE TEMP TRIGGER counts_{table}_reparent AFTER UPDATE OF {columns} ON main.{table}
             WHEN {when} BEGIN
               DELETE FROM subscription_outcome WHERE kind = 0 AND {post_predicate};
               INSERT INTO subscription_outcome SELECT * FROM subscription_post_outcome WHERE {post_predicate};
               DELETE FROM subscription_outcome WHERE kind = 1 AND {file_predicate};
               INSERT INTO subscription_outcome SELECT * FROM subscription_file_outcome WHERE {file_predicate};
             END;"
        ))?;
    }
    connection.execute_batch(
        "CREATE TEMP TRIGGER counts_post_reparent AFTER UPDATE OF run_query_id ON main.source_post_attempt
         WHEN OLD.run_query_id IS NOT NEW.run_query_id BEGIN
           DELETE FROM subscription_outcome WHERE kind = 1 AND id IN
             (SELECT file_attempt_id FROM main.source_file_attempt WHERE attempt_id = NEW.attempt_id);
           INSERT INTO subscription_outcome SELECT * FROM subscription_file_outcome WHERE id IN
             (SELECT file_attempt_id FROM main.source_file_attempt WHERE attempt_id = NEW.attempt_id);
         END;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{database::WorkPriority, Library, LibraryError};

    const FIXTURE: &str = "
        INSERT INTO subscription(subscription_id, subscription_key, name, created_at)
        VALUES (1,'one','One','now'), (2,'two','Two','now');
        INSERT INTO subscription_query(query_id, query_key, subscription_id, site_id, domain_key, query_kind, query_text)
        VALUES (1,'one',1,'test','test','tag','one'), (2,'two',2,'test','test','tag','two');
        INSERT INTO subscription_run(run_id, subscription_id, requested_by, status, created_at)
        VALUES (1,1,'user','succeeded','now'), (2,1,'user','succeeded','now'), (3,2,'user','succeeded','now');
        INSERT INTO subscription_run_query(run_query_id, run_id, query_id, status, available_at)
        VALUES (1,1,1,'succeeded','now'), (2,2,1,'succeeded','now'), (3,3,2,'succeeded','now');
        INSERT INTO source_post(source_post_id, site_id, post_key, created_at, updated_at)
        VALUES (1,'test','one','now','now');
        INSERT INTO source_item(source_item_id, source_post_id, item_key, position, state, created_at, updated_at)
        VALUES (1,1,'one',0,'ingested','now','now');
        INSERT INTO source_post_attempt(attempt_id, run_query_id, source_post_id, state, started_at)
        VALUES (1,1,1,'added','now'), (2,2,1,'added','now'), (3,3,1,'skipped','now');
        INSERT INTO source_file_attempt(file_attempt_id, attempt_id, source_item_id, state)
        VALUES (1,1,1,'retained'), (2,2,1,'retained'), (3,3,1,'duplicate');";

    fn write(library: &Library, sql: &str) {
        library
            .auxiliary_write(WorkPriority::CanonicalIngest, [], [], |tx, _| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
        check(library);
    }

    fn check(library: &Library) {
        library.auxiliary_read_consistent(WorkPriority::VisibleRead, |tx, snapshot| {
            for query in [1, 2] {
                let expected = tx.query_row(
                    "SELECT
                     (SELECT COUNT(DISTINCT a.source_post_id) FROM source_post_attempt a
                      JOIN subscription_run_query r USING(run_query_id) WHERE r.query_id=?1 AND a.state='added'),
                     (SELECT COUNT(DISTINCT f.source_item_id) FROM source_file_attempt f
                      JOIN source_post_attempt a USING(attempt_id) JOIN subscription_run_query r USING(run_query_id)
                      WHERE r.query_id=?1 AND f.state='retained'),
                     (SELECT COUNT(*) FROM subscription_run_query WHERE query_id=?1 AND status='succeeded')",
                    [query], |row| Ok(QueryCounts { posts: row.get(0)?, media: row.get(1)?, successful_runs: row.get(2)? }),
                )?;
                assert_eq!(snapshot.subscription_counts.query(query), expected);
                let expected: i64 = tx.query_row(
                    "SELECT COUNT(DISTINCT a.source_post_id) FROM source_post_attempt a
                     JOIN subscription_run_query r USING(run_query_id) JOIN subscription_query q USING(query_id)
                     WHERE q.subscription_id=?1 AND a.state='added'", [query], |row| row.get(0),
                )?;
                assert_eq!(snapshot.subscription_counts.posts(query), expected);
            }
            Ok(())
        }).unwrap();
    }

    #[test]
    fn publication_retains_distinct_outcomes_and_rolls_back_with_the_writer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite");
        let library = Library::create(&path).unwrap();
        write(&library, FIXTURE);
        let old = library.projections().snapshot();
        assert_eq!(
            old.subscription_counts.query(1),
            QueryCounts {
                posts: 1,
                media: 1,
                successful_runs: 2
            }
        );
        write(
            &library,
            "UPDATE subscription SET name='Renamed' WHERE subscription_id=1;",
        );
        assert!(std::sync::Arc::ptr_eq(
            &old.subscription_counts,
            &library.projections().snapshot().subscription_counts,
        ));
        write(
            &library,
            "UPDATE source_item SET state='deleted'; UPDATE source_item SET state='pending';
            UPDATE source_file_attempt SET state='failed' WHERE file_attempt_id=2;
            UPDATE source_post_attempt SET state='failed' WHERE attempt_id=2;",
        );
        assert_eq!(
            library
                .projections()
                .snapshot()
                .subscription_counts
                .query(1)
                .media,
            1
        );
        let revision = library.projections().snapshot().revision;
        let result = library.auxiliary_write(WorkPriority::CanonicalIngest, [], [], |tx, _| {
            tx.execute("DELETE FROM source_file_attempt", [])?;
            Err::<(), _>(LibraryError::InvalidState("rollback".into()))
        });
        assert!(result.is_err());
        assert_eq!(library.projections().snapshot().revision, revision);
        check(&library);
        write(
            &library,
            "SAVEPOINT nested; DELETE FROM subscription; ROLLBACK TO nested; RELEASE nested;",
        );
        write(
            &library,
            "UPDATE source_file_attempt SET state='retained' WHERE file_attempt_id=2;
            DELETE FROM source_file_attempt WHERE file_attempt_id=1;",
        );
        assert_eq!(
            library
                .projections()
                .snapshot()
                .subscription_counts
                .query(1)
                .media,
            1
        );
        assert_eq!(old.subscription_counts.query(1).media, 1);
        drop(library);
        let reopened = Library::open(&path).unwrap();
        check(&reopened);
        assert_eq!(
            reopened
                .projections()
                .snapshot()
                .subscription_counts
                .query(1)
                .media,
            1
        );
        write(
            &reopened,
            "DELETE FROM subscription_run WHERE subscription_id=1;",
        );
        assert_eq!(
            reopened
                .projections()
                .snapshot()
                .subscription_counts
                .query(1),
            QueryCounts::default()
        );
        drop(reopened);
        check(&Library::open(&path).unwrap());
    }

    #[test]
    fn cascading_deletes_and_reparenting_keep_query_and_subscription_attribution() {
        for delete in [
            "DELETE FROM source_item",
            "DELETE FROM source_post",
            "DELETE FROM source_post_attempt",
            "DELETE FROM subscription_run_query",
            "DELETE FROM subscription_query",
            "DELETE FROM subscription",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let library = Library::create(dir.path().join("library.sqlite")).unwrap();
            write(&library, FIXTURE);
            write(
                &library,
                "UPDATE source_post_attempt SET state='added' WHERE attempt_id=3;
                UPDATE source_file_attempt SET state='retained' WHERE file_attempt_id=3;",
            );
            write(
                &library,
                "UPDATE subscription_query SET subscription_id=2 WHERE query_id=1;",
            );
            write(
                &library,
                "UPDATE subscription_run_query SET query_id=2 WHERE run_query_id=1;",
            );
            write(
                &library,
                "DELETE FROM source_post_attempt WHERE attempt_id=1;
                UPDATE source_post_attempt SET run_query_id=1 WHERE attempt_id=2;",
            );
            write(&library, delete);
        }
    }

    #[test]
    fn unpublished_outcome_changes_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::create(dir.path().join("library.sqlite")).unwrap();
        write(&library, FIXTURE);
        assert!(library
            .database()
            .maintenance_write(WorkPriority::Maintenance, |tx| {
                tx.execute(
                    "DELETE FROM source_file_attempt WHERE file_attempt_id=2",
                    [],
                )?;
                Ok(())
            })
            .is_err());
        check(&library);
    }

    #[test]
    fn counts_and_sql_share_a_revision_while_a_writer_publishes() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::create(dir.path().join("library.sqlite")).unwrap();
        write(&library, FIXTURE);
        library
            .auxiliary_read_consistent(WorkPriority::VisibleRead, |tx, snapshot| {
                std::thread::scope(|scope| {
                    scope
                        .spawn(|| write(&library, "DELETE FROM source_file_attempt;"))
                        .join()
                        .unwrap();
                });
                let retained: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM source_file_attempt WHERE state='retained'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(retained, 2);
                assert_eq!(snapshot.subscription_counts.query(1).media, 1);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            library
                .projections()
                .snapshot()
                .subscription_counts
                .query(1)
                .media,
            0
        );
    }
}
