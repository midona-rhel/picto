/**
 * Entity target conversion helpers.
 *
 * Centralizes the conversion between frontend selection models and
 * the canonical EntityTarget shape expected by the engine.
 * This is the ONE place that builds EntityTarget payloads.
 */

import type { SelectionQuerySpec } from '../shared/types/api/core';

export interface EntityTarget {
  kind: 'entity_hashes' | 'query_results';
  entity_hashes?: string[];
  query?: {
    base_scope: unknown;
    filters?: unknown;
    sort?: unknown;
    page?: { limit: number };
  };
  excluded_entity_hashes?: string[];
}

/** Target a single entity by hash. */
export function hashTarget(hash: string): EntityTarget {
  return { kind: 'entity_hashes', entity_hashes: [hash] };
}

/** Target multiple entities by hash. */
export function hashesTarget(hashes: string[]): EntityTarget {
  return { kind: 'entity_hashes', entity_hashes: hashes };
}

/** Convert a SelectionQuerySpec to an EntityTarget. */
export function selectionToTarget(selection: SelectionQuerySpec): EntityTarget {
  if (selection.mode === 'explicit_hashes' && selection.hashes?.length) {
    return { kind: 'entity_hashes', entity_hashes: selection.hashes };
  }
  return {
    kind: 'query_results',
    query: {
      base_scope: selection.scope,
      filters: selection.filters,
      sort: selection.sort,
      page: { limit: 1000000 },
    },
  };
}
