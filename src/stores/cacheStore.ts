/**
 * Cache store — grid page data (metadata LRU), selection state, prefetch queue.
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
  blurhash: string | null;
  imported_at: string;
  dominant_color_hex: string | null;
  duration_ms: number | null;
  num_frames: number | null;
  has_audio: boolean;
  view_count: number;
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

  // Current grid page hashes (ordered)
  currentPageHashes: string[];

  // Selection state
  selectedHashes: Set<string>;
  lastSelectedHash: string | null;

  // Prefetch queue
  prefetchQueue: string[];
  prefetching: boolean;

  // Grid refresh sequence — incremented by eventBridge on grid_scopes invalidation
  gridRefreshSeq: number;

  // Metadata invalidation tracking — hashes whose metadata changed (name, rating, etc.)
  // ImageGrid subscribes and patches tiles in-place without a full grid reload.
  metadataInvalidatedHashes: Set<string>;

  // Active grid scope — used by eventBridge for scope-aware grid_scopes filtering.
  // e.g. "folder:5", "system:inbox", "system:all"
  activeGridScope: string | null;

  // Pending grid removals — hashes to optimistically remove from the grid.
  // Used by inspector actions (e.g. remove from folder) to give instant feedback.
  pendingGridRemovals: Set<string>;

  // Actions
  fetchMetadataBatch: (hashes: string[]) => Promise<ResolvedMetadata[]>;
  getMetadata: (hash: string) => ResolvedMetadata | undefined;
  setCurrentPage: (hashes: string[]) => void;
  invalidateHash: (hash: string) => void;
  invalidateAll: () => void;
  bumpGridRefresh: () => void;
  markHashInvalidated: (hash: string) => void;
  clearInvalidatedHashes: () => void;
  setActiveGridScope: (scope: string | null) => void;
  enqueueGridRemoval: (hash: string) => void;
  clearGridRemovals: () => void;

  // Selection actions
  select: (hash: string) => void;
  deselect: (hash: string) => void;
  toggleSelect: (hash: string) => void;
  selectAll: (hashes: string[]) => void;
  deselectAll: () => void;
  setSelection: (hashes: string[]) => void;

  // Prefetch
  enqueuePrefetch: (hashes: string[]) => void;
  processPrefetchQueue: () => Promise<void>;
}

export const useCacheStore = create<CacheState>((set, get) => ({
  metadataCache: new Map(),
  currentPageHashes: [],
  selectedHashes: new Set(),
  lastSelectedHash: null,
  prefetchQueue: [],
  prefetching: false,
  gridRefreshSeq: 0,
  metadataInvalidatedHashes: new Set(),
  activeGridScope: null,
  pendingGridRemovals: new Set(),

  fetchMetadataBatch: async (hashes: string[]) => {
    if (hashes.length === 0) return [];

    const cache = get().metadataCache;

    // Check which hashes need fetching
    const missing = hashes.filter((h) => !cache.has(h));

    if (missing.length > 0) {
      try {
        const resp = await api.grid.getFilesMetadataBatch(missing);
        const results: ResolvedMetadata[] = Object.values(resp.items ?? {}).map(meta => ({
          file: {
            hash: meta.file.hash,
            name: meta.file.name,
            mime: meta.file.mime,
            width: meta.file.width,
            height: meta.file.height,
            size: meta.file.size,
            status: typeof meta.file.status === 'string' ? parseInt(meta.file.status) || 0 : meta.file.status as unknown as number,
            rating: meta.file.rating,
            blurhash: meta.file.blurhash,
            imported_at: meta.file.imported_at,
            dominant_color_hex: meta.file.dominant_colors?.[0]?.hex ?? null as string | null,
            duration_ms: meta.file.duration_ms,
            num_frames: meta.file.num_frames,
            has_audio: meta.file.has_audio,
            view_count: meta.file.view_count,
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

  setCurrentPage: (hashes: string[]) => {
    set({ currentPageHashes: hashes });
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

  enqueueGridRemoval: (hash: string) => {
    set((s) => {
      const next = new Set(s.pendingGridRemovals);
      next.add(hash);
      return { pendingGridRemovals: next };
    });
  },

  clearGridRemovals: () => {
    set({ pendingGridRemovals: new Set() });
  },

  // Selection
  select: (hash: string) => {
    set((state) => {
      const newSet = new Set(state.selectedHashes);
      newSet.add(hash);
      return { selectedHashes: newSet, lastSelectedHash: hash };
    });
  },

  deselect: (hash: string) => {
    set((state) => {
      const newSet = new Set(state.selectedHashes);
      newSet.delete(hash);
      return { selectedHashes: newSet };
    });
  },

  toggleSelect: (hash: string) => {
    const state = get();
    if (state.selectedHashes.has(hash)) {
      state.deselect(hash);
    } else {
      state.select(hash);
    }
  },

  selectAll: (hashes: string[]) => {
    set({ selectedHashes: new Set(hashes), lastSelectedHash: hashes[hashes.length - 1] ?? null });
  },

  deselectAll: () => {
    set({ selectedHashes: new Set(), lastSelectedHash: null });
  },

  setSelection: (hashes: string[]) => {
    set({ selectedHashes: new Set(hashes), lastSelectedHash: hashes[hashes.length - 1] ?? null });
  },

  // Prefetch
  enqueuePrefetch: (hashes: string[]) => {
    set((state) => {
      const existing = new Set(state.prefetchQueue);
      const cache = state.metadataCache;
      const newItems = hashes.filter((h) => !existing.has(h) && !cache.has(h));
      return { prefetchQueue: [...state.prefetchQueue, ...newItems] };
    });

    // Auto-process if not already running
    if (!get().prefetching) {
      get().processPrefetchQueue();
    }
  },

  processPrefetchQueue: async () => {
    const state = get();
    if (state.prefetching || state.prefetchQueue.length === 0) return;

    set({ prefetching: true });

    try {
      while (get().prefetchQueue.length > 0) {
        const batch = get().prefetchQueue.slice(0, 50);
        set((s) => ({ prefetchQueue: s.prefetchQueue.slice(50) }));
        await get().fetchMetadataBatch(batch);
      }
    } finally {
      set({ prefetching: false });
    }
  },
}));
