import type { StateChangedEvent, TaskUpsertedEvent, TaskRemovedEvent } from '../backendState';

// ─── Event Payloads ─────────────────────────────────────────────────────────
// Typed interfaces for all backend events. Single source of truth — all
// controllers and stores should import event types from here.

export interface GroupProgressEvent {
  group_id: string;
  total: number;
  done: number;
  remaining: number;
}

export interface GroupFinishedEvent {
  group_id: string;
  status: 'succeeded' | 'failed';
  started_count?: number;
  error?: string;
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
  date_added: string;
  has_thumbnail: boolean;
}

export interface ManualImportProgressEvent {
  done: number;
  total: number;
  current_file: string;
  imported: number;
  skipped: number;
  errors: number;
}

export interface MediaExportProgressEvent {
  done: number;
  total: number;
  current_file: string;
  exported: number;
  skipped: number;
  errors: number;
}

/**
 * Core runtime event contract.
 * Keep in sync with `core/src/events.rs::event_names`.
 *
 * Authoritative events: `runtime/state_changed`, `runtime/task_upserted`,
 * `runtime/task_removed`. All domain state (subscriptions, groups) is
 * derived from task events via `applyTaskUpsert`.
 */
export interface CoreRuntimeEventPayloadMap {
  'library-closed': null;
  'zoom-factor-changed': ZoomFactorChangedEvent;
  'file-imported': FileImportedEvent;
  'manual-import-progress': ManualImportProgressEvent;
  'media-export-progress': MediaExportProgressEvent;
  'open-detail-window': OpenDetailWindowEvent;
  'duplicate-auto-merge-finished': DuplicateAutoMergeFinishedEvent;

  'log': { level: string; target: string; message: string; timestamp: string };

  // Runtime contract (authoritative)
  'runtime/state_changed': StateChangedEvent;
  'runtime/task_upserted': TaskUpsertedEvent;
  'runtime/task_removed': TaskRemovedEvent;
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

// ─── App Settings ───────────────────────────────────────────────────────────

/** Backend uses `#[serde(rename_all = "camelCase")]` — JSON keys are camelCase */
export interface AppSettings {
  gridTargetSize: number;
  gridViewMode: string;
  inspectorWidth: number;
  colorScheme: string;
  windowX?: number | null;
  windowY?: number | null;
  windowWidth: number;
  windowHeight: number;
  windowMaximized: boolean;
  gridSortField: string;
  gridSortOrder: string;
  zoomFactor?: number | null;
  duplicateDetectSimilarityPct: number;
  duplicateReviewSimilarityPct: number;
  duplicateAutoMergeSimilarityPct: number;
  duplicateAutoMergeRequireMatchingDimensions: boolean;
  duplicateAutoMergeSubscriptionsOnly: boolean;
  duplicateAutoMergeEnabled: boolean;
  subAbortThreshold: number;
  subInboxPauseLimit: number;
  subRateLimitSecs: number;
  subBatchSize: number;
  watchFolderDefaultStatus: 'inbox' | 'active';
  aiTaggerWd14Enabled: boolean;
  aiTaggerE621Enabled: boolean;
  aiTaggerAutoOnImport: boolean;
  aiThresholdGeneral: number;
  aiThresholdCharacter: number;
  aiThresholdCopyright: number;
  aiThresholdArtist: number;
  aiThresholdSpecies: number;
  aiThresholdRating: number;
  [key: string]: unknown;
}

// ─── AI Tagger ──────────────────────────────────────────────────────────────

export interface AiTaggerModelInfo {
  slug: string;
  label: string;
  onnx_url: string;
  labels_url: string;
  input_size: number;
}

export interface AiTaggerModelStatus {
  slug: string;
  label: string;
  enabled: boolean;
  downloaded: boolean;
}

export interface AiTaggerStatus {
  models: AiTaggerModelStatus[];
  gpuBackend: string | null;
  availableModels: AiTaggerModelInfo[];
}

export interface AiTagPrediction {
  tag: string;
  namespace: string;
  confidence: number;
}

export interface AiFilePrediction {
  hash: string;
  tags: AiTagPrediction[];
  error?: string;
}

export interface AiTagPredictOutput {
  predictions: AiFilePrediction[];
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

// ─── Collections ────────────────────────────────────────────────────────────

export interface CollectionInfo {
  id: number;
  name: string;
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
  tags: string[];
  image_count: number;
  total_size_bytes: number;
  mime_breakdown: CollectionMimeCount[];
  source_urls: string[];
  rating: number | null;
  date_created: string | null;
  date_modified: string | null;
  notes: string | null;
  date_added: string | null;
}

// ─── Companion ──────────────────────────────────────────────────────────────

export interface CompanionNamespaceValue {
  value: string;
  count: number;
  thumbnail_hash: string | null;
}
