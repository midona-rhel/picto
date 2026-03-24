/**
 * Filter state — one place for all grid filter controls.
 *
 * Raw filter values live here. The grid query spec is derived, not stored.
 */

import { atom } from 'jotai';

// ── Authoritative filter state ─────────────────────────────────

export const filterRatingMinAtom = atom<number | null>(null);
export const filterMimePrefixesAtom = atom<string[] | null>(null);
export const filterColorHexAtom = atom<string | null>(null);
export const filterColorAccuracyAtom = atom(50);
export const filterFolderIdsAtom = atom<number[] | null>(null);
export const filterExcludedFolderIdsAtom = atom<number[] | null>(null);
export const filterSearchTextAtom = atom('');

// ── Tag filters ────────────────────────────────────────────────

export const filterIncludeTagsAtom = atom<string[]>([]);
export const filterExcludeTagsAtom = atom<string[]>([]);
export const filterTagMatchModeAtom = atom<'all' | 'any'>('all');
export const filterFolderMatchModeAtom = atom<'all' | 'any'>('all');

// ── Derived: are any filters active? ───────────────────────────

export const hasActiveFiltersAtom = atom((get) =>
  get(filterRatingMinAtom) != null
  || get(filterMimePrefixesAtom) != null
  || get(filterColorHexAtom) != null
  || (get(filterFolderIdsAtom)?.length ?? 0) > 0
  || (get(filterExcludedFolderIdsAtom)?.length ?? 0) > 0
  || get(filterSearchTextAtom).length > 0
  || get(filterIncludeTagsAtom).length > 0
  || get(filterExcludeTagsAtom).length > 0,
);

// ── Action: clear all filters ──────────────────────────────────

export const clearAllFiltersAtom = atom(null, (_get, set) => {
  set(filterRatingMinAtom, null);
  set(filterMimePrefixesAtom, null);
  set(filterColorHexAtom, null);
  set(filterColorAccuracyAtom, 50);
  set(filterFolderIdsAtom, null);
  set(filterExcludedFolderIdsAtom, null);
  set(filterSearchTextAtom, '');
  set(filterIncludeTagsAtom, []);
  set(filterExcludeTagsAtom, []);
  set(filterTagMatchModeAtom, 'all');
  set(filterFolderMatchModeAtom, 'all');
});
