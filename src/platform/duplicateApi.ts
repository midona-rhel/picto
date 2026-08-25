import { invoke } from './ipc';
import type { DuplicateCandidate } from '../shared/types/generated/application/DuplicateCandidate';
import type { DuplicateScanResult } from '../shared/types/generated/application/DuplicateScanResult';
import type { ItemDetails } from '../shared/types/generated/application/ItemDetails';
import type { ResolutionChoice } from '../shared/types/generated/application/ResolutionChoice';
import type { ResolutionResult } from '../shared/types/generated/application/ResolutionResult';

export type DuplicateAction =
  | 'smart_merge'
  | 'keep_left'
  | 'keep_right'
  | 'not_duplicate'
  | 'keep_both';

export type DuplicatePair = DuplicateCandidate & {
  status: 'detected';
};

export interface DuplicatePairPage {
  items: DuplicatePair[];
  next_cursor: null;
  has_more: false;
  total: number;
}

export type DuplicateScanSummary = DuplicateScanResult;

export interface DuplicateResolutionResult extends ResolutionResult {
  status: 'resolved' | 'quality_ambiguous';
}

function toPair(candidate: DuplicateCandidate): DuplicatePair {
  return {
    ...candidate,
    status: 'detected',
  };
}

export function scanDuplicates(distanceThreshold = 10): Promise<DuplicateScanSummary> {
  return invoke<DuplicateScanResult>('duplicates.scan', {
    distance_threshold: distanceThreshold,
  });
}

export function getDuplicatePairs(params: { limit?: number } = {}): Promise<DuplicatePairPage> {
  return invoke<DuplicateCandidate[]>('duplicates.list', {
    limit: params.limit ?? 500,
  }).then((items) => {
    const pairs = items.map(toPair);
    return {
      items: pairs,
      next_cursor: null,
      has_more: false,
      total: pairs.length,
    };
  });
}

export function getDuplicateItemDetails(itemId: number): Promise<ItemDetails> {
  return invoke<ItemDetails>('items.details', { item_id: itemId });
}

function choiceForAction(action: DuplicateAction, pair: DuplicatePair): ResolutionChoice {
  switch (action) {
    case 'keep_both':
    case 'not_duplicate':
      return 'KeepBoth';
    case 'keep_left':
      return { KeepFile: { winner_file_id: pair.file_id_a } };
    case 'keep_right':
      return { KeepFile: { winner_file_id: pair.file_id_b } };
    case 'smart_merge':
      throw new Error('A duplicate quality choice is required.');
  }
}

export function resolveDuplicatePair(
  action: DuplicateAction,
  pair: DuplicatePair,
): Promise<DuplicateResolutionResult> {
  if (action === 'smart_merge') {
    return invoke<ResolutionResult | null>('duplicates.resolve_automatically', {
      file_id_a: pair.file_id_a,
      file_id_b: pair.file_id_b,
    }).then((result) => result == null ? {
      status: 'quality_ambiguous',
      choice: 'KeepBoth',
      affected_item_ids: [],
      freed_file_hash: null,
      receipt: { revision: 0, resources: [], item_ids: [] },
    } : { ...result, status: 'resolved' });
  }

  return invoke<ResolutionResult>('duplicates.resolve', {
    file_id_a: pair.file_id_a,
    file_id_b: pair.file_id_b,
    choice: choiceForAction(action, pair),
  }).then((result) => ({ ...result, status: 'resolved' }));
}
