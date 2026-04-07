export type CredentialType = 'username_password' | 'oauth_token' | 'cookies' | 'api_key';

export interface SubscriptionSiteInfo {
  id: string;
  name: string;
  domain: string;
  url_template: string;
  example_query: string;
  supports_query: boolean;
  supports_account: boolean;
  auth_supported: boolean;
  auth_required_for_full_access: boolean;
}

export interface SubscriptionQueryInfo {
  id: string;
  site_id: string;
  query_kind: string;
  query_text: string;
  display_name: string | null;
  notes: string | null;
  paused: boolean;
  last_check_time: string | null;
  files_found: number;
  posts_found: number;
  completed_initial_run: boolean;
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
  paused: boolean;
  group_id: string | null;
  initial_post_limit: number;
  periodic_post_limit: number;
  auto_collections: boolean;
  created_at: string;
  total_files: number;
  queries: SubscriptionQueryInfo[];
}

export interface SubscriptionProgressEvent {
  subscription_id: string;
  subscription_name: string;
  mode: string;
  group_name?: string | null;
  query_id?: string | null;
  query_name?: string | null;
  files_downloaded: number;
  files_skipped: number;
  queued_for_ingest: number;
  ingesting: number;
  ingested: number;
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
  posts_processed: number;
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
  files_downloaded: number;
  files_skipped: number;
  metadata_validated: number;
  metadata_invalid: number;
}

export interface SubscriptionIssueRecord {
  issue_id: number;
  subscription_id: number;
  query_id: number | null;
  issue_kind: string;
  status: string;
  message: string;
  detail: string | null;
  first_seen_at: string;
  last_seen_at: string;
  resolved_at: string | null;
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
  canRetry: boolean;
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

export interface PixivOAuthStartResult {
  login_url: string;
  code_verifier: string;
}

export interface PixivOAuthPopupResult {
  code: string;
  phpsessid: string | null;
}

export interface PixivOAuthExchangeResult {
  ok: boolean;
}

export interface AuthSessionBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface AuthSessionCredentialPayload {
  site_category: string;
  credential_type: CredentialType | string;
  username?: string | null;
  password?: string | null;
  cookies?: Record<string, string> | null;
  oauth_code?: string | null;
  phpsessid?: string | null;
}

export interface AuthSessionState {
  site_category: string | null;
  status: 'idle' | 'starting' | 'active' | 'loading' | 'completed' | 'error' | 'cancelled';
  title: string | null;
  current_url: string | null;
  message: string | null;
  credential: AuthSessionCredentialPayload | null;
}
