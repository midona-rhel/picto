WITH active_runs AS (
    SELECT *
    FROM subscription_run
    WHERE status IN ('pending', 'running')
),
latest_run_ids AS (
    SELECT subscription_id, MAX(run_id) AS run_id
    FROM subscription_run
    GROUP BY subscription_id
),
visible_runs AS (
    SELECT s.subscription_id, COALESCE(active.run_id, latest.run_id) AS run_id
    FROM subscription s
    LEFT JOIN active_runs active USING(subscription_id)
    LEFT JOIN latest_run_ids latest USING(subscription_id)
),
-- Posts the subscription's runs imported. Run records, not the
-- library: trashing or deleting an item does not lower it.
root_totals AS (
    SELECT query.subscription_id,
           COUNT(DISTINCT attempt.source_post_id) AS root_count
    FROM subscription_query query
    JOIN subscription_run_query run_query ON run_query.query_id = query.query_id
    JOIN source_post_attempt attempt
      ON attempt.run_query_id = run_query.run_query_id
     AND attempt.state = 'added'
    GROUP BY query.subscription_id
),
issue_totals AS (
    SELECT subscription_id, COUNT(*) AS issue_count
    FROM subscription_issue
    WHERE status = 'open'
    GROUP BY subscription_id
),
traversed_posts AS (
    SELECT srq.run_id,
           COUNT(DISTINCT attempt.attempt_id) AS posts_traversed,
           COUNT(DISTINCT CASE WHEN attempt.state = 'skipped'
                               THEN attempt.attempt_id END) AS posts_skipped
    FROM visible_runs visible
    JOIN subscription_run_query srq ON srq.run_id = visible.run_id
    LEFT JOIN source_post_attempt attempt
      ON attempt.run_query_id = srq.run_query_id
    GROUP BY srq.run_id
),
item_progress AS (
    SELECT srq.run_id,
           COUNT(DISTINCT CASE WHEN attempt.state = 'added'
                               THEN attempt.attempt_id END) AS posts_added,
           COUNT(DISTINCT file.file_attempt_id) AS discovered,
           COUNT(DISTINCT CASE WHEN file.state IN ('staged', 'retained')
                               THEN file.file_attempt_id END) AS downloaded,
           COUNT(DISTINCT CASE WHEN file.state = 'retained'
                               THEN file.file_attempt_id END) AS ingested,
           COUNT(DISTINCT CASE WHEN file.state = 'failed'
                               THEN file.file_attempt_id END) AS failed,
           0 AS deleted
    FROM visible_runs visible
    JOIN subscription_run_query srq ON srq.run_id = visible.run_id
    LEFT JOIN source_post_attempt attempt
      ON attempt.run_query_id = srq.run_query_id
    LEFT JOIN source_file_attempt file USING(attempt_id)
    GROUP BY srq.run_id
)
SELECT s.subscription_id, s.name, s.schedule, s.paused,
       s.initial_post_limit, s.periodic_post_limit, s.next_run_at,
       active.run_id,
       CASE
           WHEN active.status = 'pending'
            AND active.failure_kind IS NOT NULL
           THEN active.failure_kind
           ELSE COALESCE(active.status, latest.status)
       END,
       COALESCE(root_totals.root_count, 0),
       COALESCE(issue_totals.issue_count, 0),
       COALESCE(traversed_posts.posts_traversed, 0),
       COALESCE(item_progress.posts_added, 0),
       COALESCE(traversed_posts.posts_skipped, 0),
       COALESCE(item_progress.discovered, 0),
       COALESCE(item_progress.downloaded, 0),
       COALESCE(item_progress.ingested, 0),
       COALESCE(item_progress.failed, 0),
       COALESCE(item_progress.deleted, 0),
       (SELECT COUNT(*)
        FROM subscription_run_query query
        JOIN source_post_attempt duplicate USING(run_query_id)
        JOIN source_item existing USING(source_post_id)
        WHERE query.run_id = COALESCE(active.run_id, latest.run_id)
          AND ((duplicate.state = 'skipped'
                AND duplicate.terminal_reason = 'exact_duplicate'
                AND existing.media_item_id IS NOT NULL)
               OR EXISTS(SELECT 1 FROM source_file_attempt file
                         WHERE file.attempt_id = duplicate.attempt_id
                           AND file.source_item_id = existing.source_item_id
                           AND file.state = 'duplicate')))
FROM subscription s
LEFT JOIN active_runs active
  ON active.subscription_id = s.subscription_id
LEFT JOIN latest_run_ids latest_id
  ON latest_id.subscription_id = s.subscription_id
LEFT JOIN subscription_run latest ON latest.run_id = latest_id.run_id
LEFT JOIN root_totals USING (subscription_id)
LEFT JOIN issue_totals USING (subscription_id)
LEFT JOIN traversed_posts
  ON traversed_posts.run_id = COALESCE(active.run_id, latest.run_id)
LEFT JOIN item_progress
  ON item_progress.run_id = COALESCE(active.run_id, latest.run_id)
ORDER BY s.name, s.subscription_id
