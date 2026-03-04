import type { PtrBootstrapCounts } from './core';

// ─── Event Payloads ─────────────────────────────────────────────────────────
// Typed interfaces for all backend events. Single source of truth — all
// controllers and stores should import event types from here.

export interface StateChangedEvent {
  seq: number;
  ts: string;
  origin_command: string;
  domains: string[];
  file_hashes?: string[];
  folder_ids?: number[];
  smart_folder_ids?: number[];
  invalidate: {
    sidebar_tree?: boolean;
    grid_scopes?: string[];
    selection_summary?: boolean;
    metadata_hashes?: string[];
    view_prefs?: boolean;
  };
  compiler_batch_done?: boolean;
  sidebar_counts?: {
    all_images: number;
    inbox: number;
    trash: number;
  };
}

export interface FlowStartedEvent {
  flow_id: string;
  subscription_count: number;
}

export interface FlowProgressEvent {
  flow_id: string;
  total: number;
  done: number;
  remaining: number;
}

export interface FlowFinishedEvent {
  flow_id: string;
  status: 'succeeded' | 'failed';
  started_count?: number;
  error?: string;
}

export interface PtrBootstrapStartedEvent {
  snapshot_dir: string;
  service_id: number;
  mode: string;
}

export interface PtrBootstrapProgressEvent {
  phase: string;
  stage?: string;
  service_id?: number;
  running?: boolean;
  rows_done?: number;
  rows_total?: number;
  rows_done_stage?: number;
  rows_total_stage?: number;
  rows_per_sec?: number;
  eta_seconds?: number;
  counts?: PtrBootstrapCounts;
  updated_at?: string;
  ts?: string;
}

export interface PtrBootstrapFinishedEvent {
  success: boolean;
  dry_run?: boolean;
  service_id?: number;
  result?: unknown;
  cursor_index?: number;
  cursor_source?: string;
  delta_sync_started?: boolean;
  counts?: PtrBootstrapCounts;
}

export interface PtrBootstrapFailedEvent {
  success: false;
  error: string;
}

export interface ZoomFactorChangedEvent {
  factor: number;
}

export interface DuplicateAutoMergeFinishedEvent {
  winner_hash: string;
  loser_hash: string;
  distance: number;
  tags_merged: number;
}

// ─── PTR Status ─────────────────────────────────────────────────────────────

export interface PtrStats {
  tag_count: number;
  file_stub_count: number;
  mapping_count: number;
  sibling_count: number;
  parent_count: number;
  sync_position: number;
}

// ─── App Settings ───────────────────────────────────────────────────────────

/** Backend uses `#[serde(rename_all = "camelCase")]` — JSON keys are camelCase */
export interface AppSettings {
  gridTargetSize: number;
  gridViewMode: string;
  propertiesPanelWidth: number;
  colorScheme: string;
  windowX?: number | null;
  windowY?: number | null;
  windowWidth: number;
  windowHeight: number;
  windowMaximized: boolean;
  gridSortField: string;
  gridSortOrder: string;
  ptrServerUrl?: string | null;
  ptrAccessKey?: string | null;
  ptrEnabled: boolean;
  ptrAutoSync: boolean;
  ptrSyncSchedule: string;
  ptrLastSyncTime?: string | null;
  ptrDataPath?: string | null;
  zoomFactor?: number | null;
  duplicateDetectSimilarityPct: number;
  duplicateReviewSimilarityPct: number;
  duplicateAutoMergeSimilarityPct: number;
  duplicateAutoMergeSubscriptionsOnly: boolean;
  duplicateAutoMergeEnabled: boolean;
  subAbortThreshold: number;
  subInboxPauseLimit: number;
  subRateLimitSecs: number;
  subBatchSize: number;
  [key: string]: unknown;
}

// ─── Storage Stats ──────────────────────────────────────────────────────────

export interface StorageStats {
  file_count: number;
}

// ─── Tag Tuple ──────────────────────────────────────────────────────────────

/** Backend returns tags as [display, namespace, count] tuples */
export type TagTuple = [string, string, number];

// ─── Duplicates (per-file lookup) ───────────────────────────────────────────

export interface DuplicateInfo {
  other_hash: string;
  distance: number;
  status: string;
}

// ─── PTR Sync Perf Breakdown ────────────────────────────────────────────────

export interface PtrSyncRunPerf {
  started_at: string;
  finished_at?: string | null;
  elapsed_ms?: number | null;
  updates_processed: number;
  tags_added: number;
  siblings_added: number;
  parents_added: number;
}

export interface PtrSyncChunkPerf {
  ts: string;
  index_start: number;
  index_end: number;
  defs_insert_ms: number;
  resolve_ids_ms: number;
  content_write_ms: number;
  mapping_add_apply_ms: number;
  sibling_add_apply_ms: number;
  parent_add_apply_ms: number;
  mapping_del_apply_ms: number;
  sibling_del_apply_ms: number;
  parent_del_apply_ms: number;
  mapping_adds: number;
  sibling_adds: number;
  parent_adds: number;
  mapping_dels: number;
  sibling_dels: number;
  parent_dels: number;
  total_batches: number;
}

export interface PtrSyncPerfBreakdown {
  latest_run?: PtrSyncRunPerf | null;
  latest_chunk?: PtrSyncChunkPerf | null;
}

// ─── Collections ────────────────────────────────────────────────────────────

export interface CollectionInfo {
  id: number;
  name: string;
  description: string;
  tags: string[];
  image_count: number;
  created_at: string | null;
  updated_at: string | null;
  thumbnail_url: string | null;
}

export interface CollectionMimeCount {
  mime: string;
  count: number;
}

export interface CollectionSummary {
  id: number;
  name: string;
  description: string;
  tags: string[];
  image_count: number;
  total_size_bytes: number;
  mime_breakdown: CollectionMimeCount[];
  source_urls: string[];
  rating: number | null;
}

// ─── Review Queue ───────────────────────────────────────────────────────────

export interface ReviewQueueItem {
  hash: string;
  filename: string;
  width: number | null;
  height: number | null;
  file_size: number;
  mime: string;
  source: string;
  imported_at: string;
  has_thumbnail: boolean;
  blurhash: string | null;
  rating: number | null;
}

// ─── Companion ──────────────────────────────────────────────────────────────

export interface CompanionNamespaceValue {
  value: string;
  count: number;
  thumbnail_hash: string | null;
}
