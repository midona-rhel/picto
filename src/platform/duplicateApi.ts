import { invoke } from './ipc';

export type DuplicateAction =
  | 'smart_merge'
  | 'keep_left'
  | 'keep_right'
  | 'not_duplicate'
  | 'keep_both';

export interface DuplicatePair {
  hash_a: string;
  hash_b: string;
  distance: number;
  similarity_pct: number;
  status: string;
}

export interface DuplicatePairPage {
  items: DuplicatePair[];
  next_cursor: string | null;
  has_more: boolean;
  total: number;
}

export interface DuplicateScanSummary {
  candidates_found: number;
  pairs_inserted: number;
  reviewable_detected_total: number;
  reviewable_detected_new: number;
  total_files: number;
  files_with_phash: number;
  files_scanned: number;
  closest_distance: number | null;
}

export interface DuplicateCollectionConflict {
  winner_hash: string;
  loser_hash: string;
  winner_collection_id: number | null;
  loser_collection_id: number | null;
}

export interface DuplicateResolutionResult {
  status: 'resolved' | 'conflict' | 'quality_ambiguous';
  winner_hash: string | null;
  loser_hash: string | null;
  action: string;
  affected_folder_ids: number[];
  affected_collection_ids: number[];
  tags_merged: number;
  conflict: DuplicateCollectionConflict | null;
  blob_cleanup_pending: boolean;
  cleanup_error: string | null;
}

export function scanDuplicates(threshold?: number | null): Promise<DuplicateScanSummary> {
  return invoke<DuplicateScanSummary>('scan_duplicates', { threshold: threshold ?? null });
}

export function getDuplicatePairs(params: {
  cursor?: string | null;
  limit?: number;
  status?: string | null;
} = {}): Promise<DuplicatePairPage> {
  return invoke<DuplicatePairPage>('get_duplicate_pairs', {
    cursor: params.cursor ?? null,
    limit: params.limit ?? 100,
    status: params.status ?? 'detected',
  });
}

export function resolveDuplicatePair(
  action: DuplicateAction,
  hashA: string,
  hashB: string,
  preferredCollectionId?: number | null,
): Promise<DuplicateResolutionResult> {
  return invoke<DuplicateResolutionResult>('resolve_duplicate_pair', {
    action,
    hash_a: hashA,
    hash_b: hashB,
    preferred_collection_id: preferredCollectionId ?? null,
  });
}
