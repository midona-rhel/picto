-- Indexes that only accelerate queries and foreign-key checks; they carry no
-- meaning of their own. New libraries get them with the schema and existing
-- libraries get them at open, so adding one needs no fingerprint migration.
-- Each covers a column used in a join, a lookup, or an ON DELETE action.
CREATE INDEX IF NOT EXISTS idx_ingest_job_source_item ON ingest_job(source_item_id);
CREATE INDEX IF NOT EXISTS idx_subscription_source_post_post ON subscription_source_post(source_post_id);
CREATE INDEX IF NOT EXISTS idx_subscription_source_post_query ON subscription_source_post(query_id);
CREATE INDEX IF NOT EXISTS idx_subscription_source_post_run ON subscription_source_post(last_seen_run_id);
CREATE INDEX IF NOT EXISTS idx_subscription_run_query_query ON subscription_run_query(query_id);
CREATE INDEX IF NOT EXISTS idx_source_post_attempt_post ON source_post_attempt(source_post_id);
CREATE INDEX IF NOT EXISTS idx_source_file_attempt_item ON source_file_attempt(source_item_id);
CREATE INDEX IF NOT EXISTS idx_subscription_run_source_item_item ON subscription_run_source_item(source_item_id);
CREATE INDEX IF NOT EXISTS idx_source_attempt_root_root ON source_attempt_root(root_id);
CREATE INDEX IF NOT EXISTS idx_folder_cover_root ON folder_definition(cover_root_id);
CREATE INDEX IF NOT EXISTS idx_duplicate_pair_file_b ON duplicate_pair(file_id_b);
CREATE INDEX IF NOT EXISTS idx_duplicate_pair_winner ON duplicate_pair(winner_file_id);
CREATE INDEX IF NOT EXISTS idx_subscription_issue_subscription ON subscription_issue(subscription_id);
CREATE INDEX IF NOT EXISTS idx_subscription_issue_query ON subscription_issue(query_id);
