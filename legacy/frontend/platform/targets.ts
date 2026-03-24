/**
 * Entity target conversion helpers.
 *
 * Centralizes the conversion between frontend selection models and
 * the canonical EntityTarget shape expected by the engine.
 * This is the ONE place that builds EntityTarget payloads.
 */

import type {
  EntityTarget,
  EntityViewQuery,
} from '../shared/types/api/canonical';
import type { SelectionQuerySpec } from '../shared/types/api/core';

export type { EntityTarget } from '../shared/types/api/canonical';

/** Target a single entity by hash. */
export function hashTarget(hash: string): EntityTarget {
  return { kind: 'entity_hashes', entity_hashes: [hash] };
}

/** Target multiple entities by hash. */
export function hashesTarget(hashes: string[]): EntityTarget {
  return { kind: 'entity_hashes', entity_hashes: hashes };
}

/** Target the full result set of a query, optionally excluding specific entities. */
export function queryTarget(
  query: EntityViewQuery,
  excludedEntityHashes?: string[],
): EntityTarget {
  return {
    kind: 'query_results',
    query,
    excluded_entity_hashes: excludedEntityHashes?.length ? excludedEntityHashes : null,
  };
}

// ── Legacy conversion ────────────────────────────────────────────
// selectionToTarget converts the legacy SelectionQuerySpec to an EntityTarget.
// It passes the legacy scope/filters/sort shapes through as the query payload.
// TODO: Remove once all consumers use EntityViewQuery directly.

/** Convert a legacy SelectionQuerySpec to an EntityTarget. */
export function selectionToTarget(selection: SelectionQuerySpec): EntityTarget {
  if (selection.mode === 'explicit_hashes' && selection.hashes?.length) {
    return { kind: 'entity_hashes', entity_hashes: selection.hashes };
  }
  return {
    kind: 'query_results',
    // The legacy scope/filters/sort shapes are passed through here.
    // The backend dispatch layer handles conversion from legacy to canonical.
    query: {
      base_scope: selection.scope as EntityViewQuery['base_scope'],
      filters: selection.filters as EntityViewQuery['filters'],
      sort: selection.sort as EntityViewQuery['sort'],
      page: { limit: 1000000 },
    },
    excluded_entity_hashes: selection.excluded_hashes ?? null,
  };
}
