/**
 * Grid data state — the visible items and pagination cursor.
 *
 * Grid items are authoritative here. Display geometry (view mode, target
 * size) stays in the grid component as transient UI state — it's only
 * meaningful during the current render cycle and transitions.
 */

import { atom } from 'jotai';
import type { EntityGridItem } from '../shared/types/api/core';

// ── Authoritative grid data ────────────────────────────────────

/** Current visible grid items (from the last backend query). */
export const gridItemsAtom = atom<EntityGridItem[]>([]);

/** Opaque pagination cursor from the backend. */
export const gridCursorAtom = atom<string | null>(null);

/** Whether more pages are available. */
export const gridHasMoreAtom = atom(true);

/** Total result count from the backend (null = unknown). */
export const gridTotalCountAtom = atom<number | null>(null);

/** Loading state for grid data fetch. */
export const gridLoadingAtom = atom(false);

/** Error from the last grid query. */
export const gridErrorAtom = atom<string | null>(null);

// ── Derived ────────────────────────────────────────────────────

/** Whether the grid is empty (no items and not loading). */
export const gridEmptyAtom = atom((get) =>
  get(gridItemsAtom).length === 0 && !get(gridLoadingAtom),
);

/** Grid item count. */
export const gridItemCountAtom = atom((get) =>
  get(gridItemsAtom).length,
);

// ── Pending mutations (eager UI updates before backend confirms) ──

/** Entity hashes queued for removal from the grid (e.g. trash, delete). */
export const pendingRemovalHashesAtom = atom(new Set<string>());

/** Whether to clear the entire grid (virtual select-all trash). */
export const pendingClearAllAtom = atom(false);

/** Entities queued for insertion into the grid (e.g. undo restore). */
export const pendingInsertionsAtom = atom<EntityGridItem[]>([]);

/** Entity hashes whose metadata changed and need tile refresh. */
export const metadataChangedHashesAtom = atom(new Set<string>());
