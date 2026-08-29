export type CredentialType = 'oauth_token' | 'api_key' | 'cookies';

export interface SubscriptionSiteInfo {
  id: string;
  name: string;
  domain: string;
  credential_owner_site_id: string;
  example_query: string;
  supports_query: boolean;
  supports_account: boolean;
  auth_required_for_full_access: boolean;
  /** Site is unusable without credentials — runs are blocked, not just warned. */
  auth_strictly_required: boolean;
  credential_types: CredentialType[];
}

export interface SubscriptionQueryInfo {
  id: string;
  site_id: string;
  query_kind: string;
  query_text: string;
  display_name: string | null;
  notes: string | null;
  group_posts: boolean;
  paused: boolean;
  last_check_time: string | null;
  files_found: number;
  posts_found: number;
  completed_initial_run: boolean;
  source_history_complete: boolean;
  successful_run_count: number;
  resume_cursor: string | null;
  resume_strategy: string | null;
  last_success_at: string | null;
  last_failure_at: string | null;
  last_failure_kind: string | null;
  last_failure_message: string | null;
}

export interface SubscriptionInfo {
  id: string;
  name: string;
  schedule: SubscriptionSchedule | string;
  paused: boolean;
  run_status: string | null;
  created_at: string;
  total_items: number;
  posts_per_run: number;
  target_folder_ids: number[];
  automatic_tags: string[];
  queries: SubscriptionQueryInfo[];
}

export interface SubscriptionCover {
  file_hash: string;
  focus_x: number;
  focus_y: number;
  zoom_percent: number;
}

export type SubscriptionSchedule = 'manual' | 'daily' | 'weekly' | 'monthly';

export interface SubscriptionProgressEvent {
  subscription_id: string;
  subscription_name: string;
  run_id?: number;
  mode: string;
  query_id?: string | null;
  query_name?: string | null;
  posts_traversed: number;
  posts_added: number;
  posts_skipped: number;
  files_downloaded: number;
  gallery_total_items?: number | null;
  files_skipped: number;
  queued_for_ingest: number;
  ingesting: number;
  media_added: number;
  reused: number;
  failed_ingest: number;
  pages_fetched: number;
  metadata_validated: number;
  metadata_invalid: number;
  last_metadata_error?: string | null;
  status_text: string;
  phase?: string | null;
  current_post_id?: string | null;
  current_post_items: number;
  resume_cursor?: string | null;
  last_error?: string | null;
  finished_status?: string | null;
  failure_kind?: string | null;
  error?: string | null;
}

export interface SubscriptionRunRecord {
  run_id: number;
  subscription_id: number;
  started_at: string;
  finished_at: string | null;
  status: string;
  failure_kind: string | null;
  error_message: string | null;
  posts_traversed: number;
  posts_added: number;
  posts_skipped: number;
  media_added: number;
  files_downloaded: number;
  files_skipped: number;
  metadata_validated: number;
  metadata_invalid: number;
}

export interface SubscriptionIssueRecord {
  issue_id: number;
  issue_key: string;
  subscription_id: number;
  query_id: number | null;
  issue_kind: string;
  status: string;
  message: string;
  detail: string | null;
  first_seen_at: string;
  last_seen_at: string;
  resolved_at: string | null;
  source_item_key: string | null;
  source_post_key: string | null;
  source_post_title: string | null;
  canonical_post_url: string | null;
  media_url: string | null;
  recovery_action: string;
  next_retry_at: string | null;
}

export interface SubscriptionIssuePage {
  items: SubscriptionIssueRecord[];
  next_cursor: { last_seen_at: string; issue_id: number } | null;
  total_count: number;
}

export interface SubscriptionDownloadAttemptRecord {
  attempt_id: number;
  subscription_id: number;
  query_id: number | null;
  query_run_id: number | null;
  item_key: string;
  site_category: string | null;
  post_id: string | null;
  page_num: number | null;
  canonical_post_url: string | null;
  media_url: string | null;
  retry_url: string | null;
  retry_count: number;
  status: string;
  failure_kind: string | null;
  last_error: string | null;
  next_retry_at: string | null;
  created_at: string;
  updated_at: string;
  resolved_at: string | null;
}

export interface SubscriptionDownloadAttemptPage {
  items: SubscriptionDownloadAttemptRecord[];
  next_cursor: number | null;
  failed_post_count: number;
  retryable_post_count: number;
}

export interface FailedPostGroup {
  key: string;
  queryId: string | null;
  queryLabel: string;
  siteId: string;
  postId: string;
  canonicalPostUrl: string | null;
  mediaUrl: string | null;
  failedMembers: number;
  retryCount: number;
  status: string;
  lastError: string | null;
  nextRetryAt: string | null;
}

export interface SubscriptionBulkRetryResult {
  eligible: number;
  queued: number;
  already_queued: number;
  failed: number;
}

export interface CredentialDomain {
  site_category: string;
  credential_type: CredentialType | string;
  display_name: string | null;
  created_at: string;
}

export interface CredentialHealth {
  site_category: string;
  health_status: string;
  last_checked_at: string;
  last_error: string | null;
}

export interface AuthSessionState {
  site_category: string | null;
  status: 'idle' | 'starting' | 'active' | 'loading' | 'completed' | 'error' | 'cancelled';
  title: string | null;
  current_url: string | null;
  message: string | null;
}

export interface OnlyFansManualAuthInput {
  cookie: string;
  user_agent: string;
  x_bc: string;
}
