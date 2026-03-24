/**
 * Selection state — selected entity hashes, select-all mode, and actions.
 */

import { atom } from 'jotai';

// ── Authoritative selection state ──────────────────────────────

/** Currently selected entity hashes. Single source of truth. */
export const selectedHashesAtom = atom(new Set<string>());

/** Whether "select all" is active (virtual selection of entire scope). */
export const selectAllActiveAtom = atom(false);

/** Hashes explicitly excluded from a select-all. */
export const selectAllExcludedAtom = atom(new Set<string>());

/** The last clicked hash (for shift-click range selection). */
export const lastClickedHashAtom = atom<string | null>(null);

// ── Derived selection state ────────────────────────────────────

/** Number of selected items. */
export const selectedCountAtom = atom((get) => {
  if (get(selectAllActiveAtom)) return null; // count unknown without query
  return get(selectedHashesAtom).size;
});

/** Whether anything is selected. */
export const hasSelectionAtom = atom((get) =>
  get(selectAllActiveAtom) || get(selectedHashesAtom).size > 0,
);

/** Selected hashes as an array (for API calls that need arrays). */
export const selectedHashesArrayAtom = atom((get) =>
  [...get(selectedHashesAtom)],
);

// ── Actions (write atoms) ──────────────────────────────────────

/** Select a single hash, clearing previous selection. */
export const selectOneAtom = atom(null, (_get, set, hash: string) => {
  set(selectedHashesAtom, new Set([hash]));
  set(selectAllActiveAtom, false);
  set(selectAllExcludedAtom, new Set());
  set(lastClickedHashAtom, hash);
});

/** Toggle a hash in/out of selection. */
export const toggleSelectAtom = atom(null, (get, set, hash: string) => {
  const current = new Set(get(selectedHashesAtom));
  if (current.has(hash)) {
    current.delete(hash);
  } else {
    current.add(hash);
  }
  set(selectedHashesAtom, current);
  set(lastClickedHashAtom, hash);
});

/** Clear all selection. */
export const clearSelectionAtom = atom(null, (_get, set) => {
  set(selectedHashesAtom, new Set());
  set(selectAllActiveAtom, false);
  set(selectAllExcludedAtom, new Set());
  set(lastClickedHashAtom, null);
});

/** Activate select-all mode. */
export const selectAllAtom = atom(null, (_get, set) => {
  set(selectAllActiveAtom, true);
  set(selectAllExcludedAtom, new Set());
  set(selectedHashesAtom, new Set());
});
