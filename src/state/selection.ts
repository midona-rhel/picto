/**
 * Selection state — which entities are selected in the grid.
 *
 * Selection is hash-based (entity_hash), not index-based.
 * Single-select for now. Multi-select structure is ready but not wired.
 */

import { atom } from 'jotai';

/** The currently selected entity hash, or null if nothing is selected. */
export const selectedEntityHashAtom = atom<string | null>(null);

/** Derived: whether anything is selected. */
export const hasSelectionAtom = atom((get) => get(selectedEntityHashAtom) !== null);
