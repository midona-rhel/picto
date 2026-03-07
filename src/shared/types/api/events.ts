import type {
  PtrBootstrapCounts,
  PtrSyncPhaseChangedEvent,
  PtrSyncProgress,
  PtrSyncResult,
  SubscriptionFinishedEvent,
  SubscriptionProgressEvent,
  SubscriptionStartedEvent,
} from './core';

import type { MutationReceipt, RuntimeTask } from '../generated/runtime-contract';

// ─── Event Payloads ─────────────────────────────────────────────────────────
// Typed interfaces for all backend events. Single source of truth — all
// controllers and stores should import event types from here.

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

export interface OpenDetailWindowEvent {
  hash: string;
  width?: number;
  height?: number;
}

export interface FileImportedEvent {
  entity_id: number;
  is_collection: boolean;
  collection_item_count?: number | null;
  hash: string;
  name?: string | null;
  size: number;
  mime: string;
  width?: number | null;
  height?: number | null;
  duration_ms?: number | null;
  num_frames?: number | null;
  has_audio: boolean;
  status: string;
  rating?: number | null;
  view_count: number;
  imported_at: string;
  has_thumbnail: boolean;
  blurhash?: string | null;
}

/**
 * Core runtime event contract.
 * Keep in sync with `core/src/events.rs::event_names`.
 *
 * Authoritative events: `runtime/mutation_committed`, `runtime/task_upserted`,
 * `runtime/task_removed`. Legacy subscription/flow/PTR events are retained for
 * backward compatibility — migrate listeners to task-based model.
 */
export interface CoreRuntimeEventPayloadMap {
  // Legacy compatibility events (duplicate info carried by runtime/task_upserted)
  'subscription-started': SubscriptionStartedEvent;
  'subscription-progress': SubscriptionProgressEvent;
  'subscription-finished': SubscriptionFinishedEvent;
  'flow-started': FlowStartedEvent;
  'flow-progress': FlowProgressEvent;
  'flow-finished': FlowFinishedEvent;
  'ptr-sync-started': null;
  'ptr-sync-progress': PtrSyncProgress;
  'ptr-sync-finished': PtrSyncResult;
  'ptr-sync-phase-changed': PtrSyncPhaseChangedEvent;
  'ptr-bootstrap-started': PtrBootstrapStartedEvent;
  'ptr-bootstrap-progress': PtrBootstrapProgressEvent;
  'ptr-bootstrap-finished': PtrBootstrapFinishedEvent;
  'ptr-bootstrap-failed': PtrBootstrapFailedEvent;
  'library-closed': null;
  'zoom-factor-changed': ZoomFactorChangedEvent;
  'file-imported': FileImportedEvent;
  'open-detail-window': OpenDetailWindowEvent;
  'duplicate-auto-merge-finished': DuplicateAutoMergeFinishedEvent;

  // Runtime contract (authoritative)
  'runtime/mutation_committed': MutationReceipt;
  'runtime/task_upserted': RuntimeTask;
  'runtime/task_removed': { task_id: string };
}

/**
 * Superset map for runtime channels consumed by the renderer.
 * Includes Electron main-process channels that are not emitted by core.
 */
export interface RuntimeEventPayloadMap extends CoreRuntimeEventPayloadMap {
  'library-switching': LibrarySwitchingEvent;
  'library-switched': LibrarySwitchedEvent;
}

export type CoreRuntimeEventName = keyof CoreRuntimeEventPayloadMap;
export type RuntimeEventName = keyof RuntimeEventPayloadMap;
export type RuntimeEventPayload<K extends RuntimeEventName> = RuntimeEventPayloadMap[K];

export interface LibrarySwitchingEvent {
  path?: string;
}

export interface LibrarySwitchedEvent {
  path?: string;
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

// ─── Companion ──────────────────────────────────────────────────────────────

export interface CompanionNamespaceValue {
  value: string;
  count: number;
  thumbnail_hash: string | null;
}
