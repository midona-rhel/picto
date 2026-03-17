/**
 * Cache store — grid page data (metadata LRU) and selection state.
 *
 * Provides a centralized metadata cache for the image grid.
 * Frontend constructs asset URLs via convertFileSrc from hash.
 */

import { create } from 'zustand';
import { api } from '#desktop/api';

interface FileMetadataSlim {
  hash: string;
  name: string | null;
  mime: string;
  width: number | null;
  height: number | null;
  size: number;
  status: number;
  rating: number | null;
  imported_at: string;
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

  // Metadata invalidation tracking — hashes whose metadata changed (name, rating, etc.)
  // ImageGrid subscribes and patches tiles in-place without a full grid reload.
  metadataInvalidatedHashes: Set<string>;

  // Active grid scope — used by gridRefresher for scope-aware grid_scopes filtering.
  // e.g. "folder:5", "system:inbox", "system:all"
  activeGridScope: string | null;

  // Actions
  fetchMetadataBatch: (hashes: string[]) => Promise<ResolvedMetadata[]>;
  getMetadata: (hash: string) => ResolvedMetadata | undefined;
  invalidateHash: (hash: string) => void;
  invalidateAll: () => void;
  bumpGridRefresh: () => void;
  markHashInvalidated: (hash: string) => void;
  clearInvalidatedHashes: () => void;
  setActiveGridScope: (scope: string | null) => void;

}

export const useGridMetadataStore = create<CacheState>((set, get) => ({
  metadataCache: new Map(),
  gridRefreshSeq: 0,
  metadataInvalidatedHashes: new Set(),
  activeGridScope: null,

  fetchMetadataBatch: async (hashes: string[]) => {
    if (hashes.length === 0) return [];

    const cache = get().metadataCache;

    // Check which hashes need fetching
    const missing = hashes.filter((h) => !cache.has(h));

    if (missing.length > 0) {
      try {
        const resp = await api.grid.getEntitiesMetadataBatch(missing);
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
            imported_at: meta.entity.imported_at,
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

  invalidateHash: (hash: string) => {
    set((state) => {
      const newCache = new Map(state.metadataCache);
      newCache.delete(hash);
      return { metadataCache: newCache };
    });
  },

  invalidateAll: () => {
    set({ metadataCache: new Map() });
  },

  bumpGridRefresh: () => {
    set((s) => ({ gridRefreshSeq: s.gridRefreshSeq + 1 }));
  },

  markHashInvalidated: (hash: string) => {
    set((s) => {
      const next = new Set(s.metadataInvalidatedHashes);
      next.add(hash);
      return { metadataInvalidatedHashes: next };
    });
  },

  clearInvalidatedHashes: () => {
    set({ metadataInvalidatedHashes: new Set() });
  },

  setActiveGridScope: (scope: string | null) => {
    set({ activeGridScope: scope });
  },


}));
