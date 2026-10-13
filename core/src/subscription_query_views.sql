SELECT q.subscription_id, q.query_id, q.site_id, q.query_kind, q.query_text,
       q.display_name, q.notes, q.group_posts, q.paused, q.initial_run_complete,
       COALESCE(q.resume_cursor = '', 0),
       q.last_success_at, q.last_failure_at, q.last_failure_kind,
       q.last_failure_message
FROM subscription_query q
ORDER BY q.subscription_id, q.query_id
