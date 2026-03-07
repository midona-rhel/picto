import { create } from 'zustand';

export type MimeFilterKey = 'images' | 'videos' | 'gifs' | 'audio' | 'other';

export interface FolderFilterState {
  includes: Map<number, string>; // folderId → name
  excludes: Map<number, string>; // folderId → name
}

export type FilterLogicMode = 'OR' | 'AND' | 'EQUAL';

export interface FilterState {
  filterBarOpen: boolean;
  ratingFilter: number | null;     // null = any, 1–5 = minimum stars
  mimeFilter: Set<MimeFilterKey>;  // empty = all types
  colorFilter: string | null;      // null = any, hex string when active
  colorAccuracy: number;           // 1–30, lower = stricter match (default: 20)
  searchText: string;
  folderFilter: FolderFilterState;
  folderFilterMode: FilterLogicMode;

  // Actions
  toggleFilterBar: () => void;
  setFilterBarOpen: (open: boolean) => void;
  setRatingFilter: (rating: number | null) => void;
  toggleMimeFilter: (key: MimeFilterKey) => void;
  setColorFilter: (hex: string | null) => void;
  setColorAccuracy: (accuracy: number) => void;
  setSearchText: (text: string) => void;
  clearMimeFilter: () => void;
  /** Left-click: toggle include. If currently excluded, remove exclude + add include. */
  includeFolderFilter: (id: number, name: string) => void;
  /** Right-click: toggle exclude. If currently included, remove include + add exclude. */
  excludeFolderFilter: (id: number, name: string) => void;
  setFolderFilterMode: (mode: FilterLogicMode) => void;
  clearFolderFilter: () => void;
  clearAllFilters: () => void;
}

export const useFilterStore = create<FilterState>((set) => ({
  filterBarOpen: false,
  ratingFilter: null,
  mimeFilter: new Set(),
  colorFilter: null,
  colorAccuracy: 20,
  searchText: '',
  folderFilter: { includes: new Map(), excludes: new Map() },
  folderFilterMode: 'OR',

  toggleFilterBar: () => set((s) => ({ filterBarOpen: !s.filterBarOpen })),
  setFilterBarOpen: (open) => set({ filterBarOpen: open }),

  setRatingFilter: (rating) => set({ ratingFilter: rating }),

  toggleMimeFilter: (key) =>
    set((s) => {
      const next = new Set(s.mimeFilter);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return { mimeFilter: next };
    }),

  setColorFilter: (hex) => {
    if (hex === null) {
      set({ colorFilter: null });
      return;
    }
    const normalized = hex.trim().toUpperCase();
    if (/^#[0-9A-F]{6}$/.test(normalized)) {
      set({ colorFilter: normalized });
    }
  },
  setColorAccuracy: (accuracy) => set({ colorAccuracy: Math.max(1, Math.min(30, accuracy)) }),

  setSearchText: (text) => set({ searchText: text }),

  clearMimeFilter: () => set({ mimeFilter: new Set() }),

  includeFolderFilter: (id, name) =>
    set((s) => {
      const includes = new Map(s.folderFilter.includes);
      const excludes = new Map(s.folderFilter.excludes);
      excludes.delete(id);
      if (includes.has(id)) includes.delete(id);
      else includes.set(id, name);
      return { folderFilter: { includes, excludes } };
    }),

  excludeFolderFilter: (id, name) =>
    set((s) => {
      const includes = new Map(s.folderFilter.includes);
      const excludes = new Map(s.folderFilter.excludes);
      includes.delete(id);
      if (excludes.has(id)) excludes.delete(id);
      else excludes.set(id, name);
      return { folderFilter: { includes, excludes } };
    }),

  setFolderFilterMode: (mode) => set({ folderFilterMode: mode }),

  clearFolderFilter: () => set({ folderFilter: { includes: new Map(), excludes: new Map() } }),

  clearAllFilters: () =>
    set({
      ratingFilter: null,
      mimeFilter: new Set(),
      colorFilter: null,
      colorAccuracy: 20,
      searchText: '',
      folderFilter: { includes: new Map(), excludes: new Map() },
      folderFilterMode: 'OR',
    }),
}));

/** Derived: count of active filter dimensions (not counting search text). */
export function useActiveFilterCount(): number {
  const rating = useFilterStore((s) => s.ratingFilter);
  const mimeSize = useFilterStore((s) => s.mimeFilter.size);
  const color = useFilterStore((s) => s.colorFilter);
  const folderIncludes = useFilterStore((s) => s.folderFilter.includes.size);
  const folderExcludes = useFilterStore((s) => s.folderFilter.excludes.size);
  let count = 0;
  if (rating !== null) count++;
  if (mimeSize > 0) count++;
  if (color !== null) count++;
  if (folderIncludes > 0 || folderExcludes > 0) count++;
  return count;
}

/** Convert mimeFilter Set to MIME prefix strings for backend query. */
export function mimeFilterToPrefixes(filter: Set<MimeFilterKey>): string[] | null {
  if (filter.size === 0) return null;
  const prefixes: string[] = [];
  if (filter.has('images')) prefixes.push('image/');
  if (filter.has('videos')) prefixes.push('video/');
  if (filter.has('gifs')) prefixes.push('image/gif');
  if (filter.has('audio')) prefixes.push('audio/');
  // 'other' is handled by the backend as "not matching any known prefix"
  return prefixes.length > 0 ? prefixes : null;
}
