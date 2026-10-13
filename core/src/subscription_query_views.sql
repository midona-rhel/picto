WITH added_source_posts AS (
    SELECT run_query.query_id, attempt.source_post_id
    FROM subscription_run_query run_query
    JOIN source_post_attempt attempt USING(run_query_id)
    WHERE attempt.state = 'added'
    GROUP BY run_query.query_id, attempt.source_post_id
),
post_counts AS (
    SELECT query_id, COUNT(*) AS post_count
    FROM added_source_posts
    GROUP BY query_id
),
-- Retained outcomes belong to this query, unlike mutable source_item
-- state shared by all queries. Deletion and rediscovery cannot erase them.
media_counts AS (
    SELECT run_query.query_id, COUNT(DISTINCT file.source_item_id) AS media_count
    FROM source_file_attempt file
    JOIN source_post_attempt attempt USING(attempt_id)
    JOIN subscription_run_query run_query USING(run_query_id)
    WHERE file.state = 'retained'
    GROUP BY run_query.query_id
),
successful_runs AS (
    SELECT query_id, COUNT(*) AS run_count
    FROM subscription_run_query
    WHERE status = 'succeeded'
    GROUP BY query_id
)
SELECT q.subscription_id, q.query_id, q.site_id, q.query_kind, q.query_text,
       q.display_name, q.notes, q.group_posts, q.paused, q.initial_run_complete,
       COALESCE(q.resume_cursor = '', 0),
       q.last_success_at, q.last_failure_at, q.last_failure_kind,
       q.last_failure_message,
       COALESCE(post_counts.post_count, 0),
       COALESCE(media_counts.media_count, 0),
       COALESCE(successful_runs.run_count, 0)
FROM subscription_query q
LEFT JOIN post_counts ON post_counts.query_id = q.query_id
LEFT JOIN media_counts ON media_counts.query_id = q.query_id
LEFT JOIN successful_runs ON successful_runs.query_id = q.query_id
ORDER BY q.subscription_id, q.query_id
