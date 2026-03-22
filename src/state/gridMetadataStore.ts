/**
 * Cache store — grid page data (metadata LRU) and selection state.
 *
 * Provides a centralized metadata cache for the image grid.
 * Frontend constructs asset URLs via convertFileSrc from hash.
 */

import { create } from 'zustand';
import { filesController } from '../controllers/filesController';
import type { EntitySlim } from '../shared/types/api';

interface FileMetadataSlim {
  hash: string;
  name: string | null;
  mime: string;
  width: number | null;
  height: number | null;
  size: number;
  status: number;
  rating: number | null;
  date_added: string;
  dominant_color_hex: string | null;
  duration_ms: number | null;
  num_frames: number | null;
  has_audio: boolean;
}

interface ResolvedMetadata {
  file: FileMetadataSlim;
  tags: Array<{
    tag_id: number;
    namespace: string;
    subtag: string;
    source: string;
    display_ns: string | null;
    display_st: string | null;
  }>;
}

const MAX_CACHE_SIZE = 5000;

interface CacheState {
  // Metadata cache (hash → resolved metadata)
  metadataCache: Map<string, ResolvedMetadata>;

  // Grid refresh sequence — incremented by gridRefresher on grid_scopes invalidation
  gridRefreshSeq: number;

  // Metadata change tracking — hashes whose metadata changed (name, rating, etc.)
  // ImageGrid subscribes and patches tiles in-place without a full grid reload.
  metadataInvalidatedHashes: Set<string>;

  // Pending removals — hashes that should be immediately removed from the visible
  // grid (e.g. after trash/delete). The grid subscribes and applies FILTER_IMAGES.
  // When pendingClearAll is true, the entire dataset is cleared (virtual select-all ops).
  pendingRemovals: Set<string>;
  pendingClearAll: boolean;

  // Pending insertions — entities that should be inserted/updated in the visible
  // grid (e.g. restore to active while viewing system:all).
  pendingInsertions: EntitySlim[];

  // Active grid scope — used by the grid refresh applier for scope-aware grid target filtering.
  // e.g. "folder:5", "system:inbox", "system:all"
  activeGridScope: string | null;

  // Actions
  fetchMetadataBatch: (hashes: string[]) => Promise<ResolvedMetadata[]>;
  getMetadata: (hash: string) => ResolvedMetadata | undefined;
  dropCachedMetadata: (hash: string) => void;
  markMetadataChanged: (hash: string) => void;
  clearChangedMetadataMarks: () => void;
  queueRemovals: (hashes: string[]) => void;
  drainRemovals: () => { hashes: Set<string>; clearAll: boolean };
  queueInsertions: (entities: EntitySlim[]) => void;
  drainInsertions: () => EntitySlim[];
  setActiveGridScope: (scope: string | null) => void;
  /** Scoped grid replace — only bumps refresh if the given scope matches the active scope. */
  requestScopedReplace: (scope: string) => void;

}

export const useGridMetadataStore = create<CacheState>((set, get) => ({
  metadataCache: new Map(),
  gridRefreshSeq: 0,
  metadataInvalidatedHashes: new Set(),
  pendingRemovals: new Set(),
  pendingClearAll: false,
  pendingInsertions: [],
  activeGridScope: null,

  fetchMetadataBatch: async (hashes: string[]) => {
    if (hashes.length === 0) return [];

    const cache = get().metadataCache;

    // Check which hashes need fetching
    const missing = hashes.filter((h) => !cache.has(h));

    if (missing.length > 0) {
      try {
        const resp = await filesController.getMetadataBatch(missing);
        const results: ResolvedMetadata[] = Object.values(resp.items ?? {}).map(meta => ({
          file: {
            hash: meta.entity.hash,
            name: meta.entity.name,
            mime: meta.entity.mime,
            width: meta.entity.width,
            height: meta.entity.height,
            size: meta.entity.size,
            status: typeof meta.entity.status === 'string' ? parseInt(meta.entity.status) || 0 : meta.entity.status as unknown as number,
            rating: meta.entity.rating,
            date_added: meta.entity.date_added,
            dominant_color_hex: meta.entity.dominant_colors?.[0]?.hex ?? null as string | null,
            duration_ms: meta.entity.duration_ms,
            num_frames: meta.entity.num_frames,
            has_audio: meta.entity.has_audio,
          },
          tags: meta.tags.map(t => ({
            tag_id: 0,
            namespace: t.namespace ?? '',
            subtag: t.subtag ?? '',
            source: t.source ?? '',
            display_ns: null as string | null,
            display_st: null as string | null,
          })),
        }));

        set((state) => {
          const newCache = new Map(state.metadataCache);
          for (const result of results) {
            newCache.set(result.file.hash, result);
          }

          // Evict oldest entries if cache too large
          if (newCache.size > MAX_CACHE_SIZE) {
            const keys = Array.from(newCache.keys());
            const toEvict = keys.slice(0, newCache.size - MAX_CACHE_SIZE);
            for (const key of toEvict) {
              newCache.delete(key);
            }
          }

          return { metadataCache: newCache };
        });
      } catch (e) {
        console.error('Failed to fetch metadata batch:', e);
      }
    }

    // Return all requested from cache
    const updatedCache = get().metadataCache;
    return hashes
      .map((h) => updatedCache.get(h))
      .filter((m): m is ResolvedMetadata => m !== undefined);
  },

  getMetadata: (hash: string) => {
    return get().metadataCache.get(hash);
  },

  dropCachedMetadata: (hash: string) => {
    set((state) => {
      const newCache = new Map(state.metadataCache);
      newCache.delete(hash);
      return { metadataCache: newCache };
    });
  },

  markMetadataChanged: (hash: string) => {
    set((s) => {
      const next = new Set(s.metadataInvalidatedHashes);
      next.add(hash);
      return { metadataInvalidatedHashes: next };
    });
  },

  clearChangedMetadataMarks: () => {
    set({ metadataInvalidatedHashes: new Set() });
  },

  queueRemovals: (hashes: string[]) => {
    set((s) => {
      const next = new Set(s.pendingRemovals);
      for (const hash of hashes) next.add(hash);
      return { pendingRemovals: next };
    });
  },

  drainRemovals: () => {
    const { pendingRemovals, pendingClearAll } = get();
    if (pendingRemovals.size > 0 || pendingClearAll) {
      set({ pendingRemovals: new Set(), pendingClearAll: false });
    }
    return { hashes: pendingRemovals, clearAll: pendingClearAll };
  },

  queueInsertions: (entities: EntitySlim[]) => {
    set((s) => ({ pendingInsertions: [...s.pendingInsertions, ...entities] }));
  },

  drainInsertions: () => {
    const current = get().pendingInsertions;
    if (current.length > 0) set({ pendingInsertions: [] });
    return current;
  },

  setActiveGridScope: (scope: string | null) => {
    set({ activeGridScope: scope });
  },

  requestScopedReplace: (scope: string) => {
    if (get().activeGridScope === scope) {
      set((s) => ({ gridRefreshSeq: s.gridRefreshSeq + 1 }));
    }
  },

}));
