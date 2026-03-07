/**
 * Centralized API types — single source of truth for all backend command
 * request/response interfaces and event payloads.
 *
 * Organized by domain. Every invoke<T>() call should reference a type from here.
 */

// ─── Grid & Pagination ──────────────────────────────────────────────────────

export interface EntitySlim {
  entity_id?: number;
  is_collection?: boolean;
  collection_item_count?: number | null;
  hash: string;
  name: string | null;
  mime: string;
  size: number;
  width: number | null;
  height: number | null;
  duration_ms: number | null;
  num_frames: number | null;
  has_audio: boolean;
  status: string;
  rating: number | null;
  view_count: number;
  source_urls: string[] | null;
  imported_at: string;
  has_thumbnail: boolean;
  blurhash?: string | null;
  tags?: string[];
  dominant_colors?: { hex: string; l: number; a: number; b: number }[] | null;
  notes?: Record<string, string> | null;
}

export interface EntityDetails extends EntitySlim {}

export interface GridPageSlimResponse {
  items: EntitySlim[];
  next_cursor: string | null;
  has_more: boolean;
  total_count: number | null;
}

export interface GridPageSlimQuery {
  limit: number;
  cursor: string | null;
  sortField: string;
  sortOrder: string;
  smartFolderPredicate?: unknown | null;
  searchTags?: string[] | null;
  searchExcludedTags?: string[] | null;
  tagMatchMode?: 'all' | 'any' | 'exact' | null;
  status?: string | null;
  folderIds?: number[] | null;
  excludedFolderIds?: number[] | null;
  folderMatchMode?: 'all' | 'any' | 'exact' | null;
  collectionEntityId?: number | null;
  ratingMin?: number | null;
  mimePrefixes?: string[] | null;
  colorHex?: string | null;
  colorAccuracy?: number | null;
  searchText?: string | null;
  randomSeed?: number | null;
}

// ─── File Metadata ───────────────────────────────────────────────────────────

export interface ResolvedTagInfo {
  raw_tag: string;
  display_tag: string;
  namespace: string;
  subtag: string;
  source: string;
  read_only: boolean;
}

export interface EntityAllMetadata {
  file: {
    hash: string;
    name: string | null;
    size: number;
    mime: string;
    width: number | null;
    height: number | null;
    duration_ms: number | null;
    num_frames: number | null;
    has_audio: boolean;
    status: string;
    rating: number | null;
    view_count: number;
    source_urls: string[] | null;
    imported_at: string;
    has_thumbnail: boolean;
    blurhash: string | null;
    dominant_colors: { hex: string; l: number; a: number; b: number }[] | null;
    notes: Record<string, string> | null;
  };
  tags: ResolvedTagInfo[];
  parent_tags: { namespace: string; subtag: string; display: string; read_only: boolean }[];
}

export interface EntityMetadataBatchResponse {
  items: Record<string, EntityAllMetadata>;
  missing: string[];
  generated_at: string;
}

export interface EnsureThumbnailResponse {
  regenerated_thumbnail: boolean;
  generated_blurhash: boolean;
  has_thumbnail: boolean;
  blurhash?: string | null;
}

export interface ReanalyzeFileColorsResponse {
  colors_extracted: number;
  dominant_color_hex: string | null;
}

export interface ImportResult {
  imported: string[];
  skipped: string[];
  errors: string[];
}

export interface BackfillBlurhashResult {
  processed: number;
  regenerated_thumbnails: number;
  generated_blurhashes: number;
  remaining: number;
}

// ─── Tags ────────────────────────────────────────────────────────────────────

export interface TagDisplay {
  display: string;
}

export interface TagSearchResult {
  tag_id: number;
  namespace: string;
  subtag: string;
  display: string;
  file_count: number;
  read_only: boolean;
}

export interface TagRecord {
  tag_id: number;
  namespace: string;
  subtag: string;
  file_count: number;
}

export interface NamespaceSummary {
  namespace: string;
  count: number;
}

export interface TagAlias {
  from: string;
  to: string;
}

export interface TagRelation {
  tag_id: number;
  namespace: string;
  subtag: string;
  display: string;
  relation?: string;
}

export interface RenameTagResult {
  affected_files: number;
  merged_into: boolean;
}

export interface DeleteTagResult {
  affected_files: number;
}

export interface NormalizeNamespacesResult {
  tags_rewritten: number;
  tags_merged: number;
  affected_files: number;
}

// ─── Selection ───────────────────────────────────────────────────────────────

export type SelectionMode = 'explicit_hashes' | 'all_results';

export interface SelectionQuerySpec {
  mode: SelectionMode;
  hashes?: string[] | null;
  search_tags?: string[] | null;
  search_excluded_tags?: string[] | null;
  tag_match_mode?: 'all' | 'any' | 'exact' | null;
  smart_folder_predicate?: SmartFolderPredicate | null;
  smart_folder_sort_field?: string | null;
  smart_folder_sort_order?: string | null;
  sort_field?: string | null;
  sort_order?: string | null;
  excluded_hashes?: string[] | null;
  included_hashes?: string[] | null;
  status?: string | null;
  folder_ids?: number[] | null;
  excluded_folder_ids?: number[] | null;
  folder_match_mode?: 'all' | 'any' | 'exact' | null;
}

export interface SelectionTagCount {
  tag: string;
  count: number;
}

export interface SelectionSummary {
  total_count: number;
  selected_count: number;
  sample_hashes: string[];
  shared_tags: SelectionTagCount[];
  top_tags: SelectionTagCount[];
  stats: {
    total_size_bytes?: number | null;
    mime_counts?: Record<string, number> | null;
    rating_stats?: { min?: number; max?: number; shared?: number | null } | null;
  };
  pending: boolean;
  generated_at: string;
}

// ─── Folders ─────────────────────────────────────────────────────────────────

export interface Folder {
  folder_id: number;
  name: string;
  parent_id: number | null;
  icon: string | null;
  color: string | null;
  auto_tags: string[];
  sort_order: number | null;
  created_at: string | null;
  updated_at: string | null;
}

export interface FolderMembership {
  folder_id: number;
  folder_name: string;
}

export interface FolderReorderMove {
  hash: string;
  position_rank: number;
}

// ─── Smart Folders ───────────────────────────────────────────────────────────

export interface SmartRule {
  field: string;
  op: string;
  value?: string | number | boolean;
  value2?: string | number;
  values?: string[];
}

export interface SmartRuleGroup {
  match_mode: 'all' | 'any';
  negate: boolean;
  rules: SmartRule[];
}

export interface SmartFolderPredicate {
  groups: SmartRuleGroup[];
}

export interface SmartFolder {
  id?: string;
  name: string;
  icon?: string | null;
  color?: string | null;
  predicate: SmartFolderPredicate;
  sort_field?: string | null;
  sort_order?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
}

// ─── Duplicates ──────────────────────────────────────────────────────────────

export interface ScanDuplicatesResult {
  candidates_found: number;
  pairs_inserted: number;
  reviewable_detected_total: number;
  reviewable_detected_new: number;
  total_files: number;
  files_with_phash: number;
  closest_distance: number | null;
}

// ─── Subscriptions & Flows (CRUD) ────────────────────────────────────────────

export interface SubscriptionQueryInfo {
  id: string;
  query_text: string;
  display_name: string | null;
  paused: boolean;
  last_check_time: string | null;
  files_found: number;
  completed_initial_run: boolean;
  resume_cursor?: string | null;
  resume_strategy?: string | null;
}

export interface SubscriptionInfo {
  id: string;
  name: string;
  site_id: string;
  paused: boolean;
  flow_id: string | null;
  initial_file_limit: number;
  periodic_file_limit: number;
  created_at: string;
  total_files: number;
  queries: SubscriptionQueryInfo[];
}

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

export interface SiteMetadataSchema {
  site_id: string;
  required_raw_keys: string[];
  required_normalized_fields: string[];
  namespace_mapping: Record<string, string>;
  failure_policy: string;
}

export interface SiteMetadataValidationResult {
  valid: boolean;
  missing_required_fields: string[];
  invalid_fields: string[];
  normalized_preview: Record<string, unknown> | null;
  warnings: string[];
}

export type CredentialType = 'username_password' | 'cookies' | 'api_key' | 'oauth_token';

export interface CredentialDomain {
  site_category: string;
  credential_type: CredentialType;
  display_name: string | null;
  created_at: string;
}

export type CredentialHealthStatus =
  | 'unknown'
  | 'valid'
  | 'missing'
  | 'unauthorized'
  | 'expired'
  | 'error';

export interface CredentialHealth {
  site_category: string;
  health_status: CredentialHealthStatus;
  last_checked_at: string;
  last_error: string | null;
}

export interface FlowInfo {
  id: string;
  name: string;
  schedule: string;
  created_at: string;
  total_files: number;
  subscriptions: SubscriptionInfo[];
}

// ─── Subscriptions & Flows (Events) ─────────────────────────────────────────

export interface SubscriptionProgressEvent {
  subscription_id: string;
  subscription_name: string;
  mode: string;
  query_id?: string;
  query_name?: string;
  files_downloaded: number;
  files_skipped: number;
  pages_fetched: number;
  metadata_validated: number;
  metadata_invalid: number;
  last_metadata_error?: string | null;
  status_text: string;
}

export interface SubscriptionStartedEvent {
  subscription_id: string;
  subscription_name: string;
  mode: 'subscription' | 'query';
  query_id?: string;
  query_name?: string;
}

export interface SubscriptionFinishedEvent {
  subscription_id: string;
  subscription_name: string;
  mode: 'subscription' | 'query';
  query_id?: string;
  query_name?: string;
  status: 'succeeded' | 'failed' | 'cancelled';
  files_downloaded: number;
  files_skipped: number;
  errors_count: number;
  error?: string | null;
  failure_kind?: string | null;
  metadata_validated: number;
  metadata_invalid: number;
  last_metadata_error?: string | null;
}

// ─── PTR (Public Tag Repository) ─────────────────────────────────────────────

export interface PtrSyncProgress {
  updates_total: number;
  updates_processed: number;
  tags_added: number;
  siblings_added: number;
  parents_added: number;
  current_update_index: number;
  latest_server_index: number;
  starting_index: number;
  phase: string;
  heartbeat: boolean;
  elapsed_ms: number;
}

export interface PtrBootstrapCounts {
  hash_defs: number;
  tag_defs: number;
  mappings: number;
  siblings: number;
  parents: number;
  max_update_index: number;
}

export interface PtrBootstrapResult {
  service_id: number;
  counts: PtrBootstrapCounts;
  projected_import_seconds: number;
  snapshot_dir: string;
}

export interface PtrBootstrapStatus {
  running: boolean;
  phase: string;
  stage?: string;
  mode: string;
  service_id?: number;
  started_at?: string;
  updated_at?: string;
  last_error?: string;
  rows_total?: number;
  rows_done?: number;
  rows_done_stage?: number;
  rows_total_stage?: number;
  rows_per_sec?: number;
  eta_seconds?: number;
  checkpoint?: {
    phase: string;
    last_hash_id: number;
    last_tag_id: number;
    last_service_hash_id: number;
  };
  last_result?: {
    service_id: number;
    counts: PtrBootstrapCounts;
    cursor_index: number;
    snapshot_dir: string;
  };
  dry_run_result?: PtrBootstrapResult;
}

export interface PtrSyncResult {
  success: boolean;
  error?: string;
  updates_processed?: number;
  tags_added?: number;
}

export interface PtrCompactIndexStatus {
  running: boolean;
  stage: string;
  rows_done_stage: number;
  rows_total_stage: number;
  rows_per_sec: number;
  snapshot_dir?: string;
  service_id?: number;
  snapshot_max_index?: number;
  updated_at?: string;
  checkpoint: {
    phase: string;
    last_hash_id: number;
    last_tag_id: number;
    last_service_hash_id: number;
  };
}

export interface PtrSyncPhaseChangedEvent {
  phase: string;
  current_update_index?: number;
  ts?: string;
}

// ─── View Preferences ────────────────────────────────────────────────────────

export interface ViewPrefsDto {
  scope_key: string;
  sort_field: string | null;
  sort_order: string | null;
  view_mode: string | null;
  target_size: number | null;
  show_name: boolean | null;
  show_resolution: boolean | null;
  show_extension: boolean | null;
  show_label: boolean | null;
  thumbnail_fit: string | null;
}

export interface ViewPrefsPatch {
  sort_field?: string | null;
  sort_order?: string | null;
  view_mode?: string | null;
  target_size?: number | null;
  show_name?: boolean | null;
  show_resolution?: boolean | null;
  show_extension?: boolean | null;
  show_label?: boolean | null;
  thumbnail_fit?: string | null;
}

// ─── Settings ────────────────────────────────────────────────────────────────

export interface FileStats {
  total: number;
  inbox: number;
  active: number;
  trash: number;
  total_size: number;
}

export interface PercentileSnapshot {
  count: number;
  p50_ms: number;
  p95_ms: number;
  p99_ms: number;
  max_ms: number;
  avg_ms: number;
}

export interface MetadataBatchLatest {
  total_ms: number;
  local_ms: number;
  ptr_ms: number;
  merge_ms: number;
  req_hashes: number;
  local_hits: number;
  ptr_lookup: number;
  ptr_hits: number;
  missing: number;
  ts: string;
}

export interface PerfSnapshot {
  grid_page_slim: PercentileSnapshot;
  files_metadata_batch: PercentileSnapshot;
  sidebar_tree: PercentileSnapshot;
  selection_summary: PercentileSnapshot;
  metadata_batch_latest?: MetadataBatchLatest | null;
  projection_corruption_count: number;
}

export interface SloStat {
  available: boolean;
  p50_ms: number;
  p95_ms: number;
  p99_ms: number;
  target_p50_ms: number;
  target_p95_ms: number;
  target_p99_ms: number;
  pass_p50: boolean;
  pass_p95: boolean;
  pass_p99: boolean;
}

export interface PerfSloResult {
  pass: boolean;
  click_metadata: SloStat;
  grid_first_page: SloStat;
  sidebar_tree: SloStat;
  selection_summary: SloStat;
  missing_metrics: string[];
}

// ─── Duplicates ─────────────────────────────────────────────────────────────

export interface DuplicateSettings {
  duplicateDetectSimilarityPct: number;
  duplicateReviewSimilarityPct: number;
  duplicateAutoMergeSimilarityPct: number;
  duplicateAutoMergeSubscriptionsOnly: boolean;
  duplicateAutoMergeEnabled: boolean;
}

export interface DuplicatePairDto {
  hash_a: string;
  hash_b: string;
  distance: number;
  similarity_pct: number;
  status: string;
}

export interface DuplicatePairsResponse {
  items: DuplicatePairDto[];
  next_cursor: string | null;
  has_more: boolean;
  total: number;
}

export type ResolveDuplicateAction =
  | 'smart_merge'
  | 'keep_left'
  | 'keep_right'
  | 'not_duplicate'
  | 'keep_both';

export interface SmartMergeResult {
  winner_hash: string;
  loser_hash: string;
  tags_merged: number;
}

// ─── Color Search ────────────────────────────────────────────────────────────

export interface ColorSearchResult {
  hash: string;
  distance: number;
}

// ─── Library ─────────────────────────────────────────────────────────────────

export interface LibraryInfo {
  path: string;
  name: string;
  file_count: number;
}
