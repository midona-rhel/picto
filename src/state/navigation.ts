/**
 * Navigation state — which sidebar scope is active.
 *
 * The grid area reads this to know what to query.
 * Sidebar rows read this to show the active highlight.
 */

import { atom } from 'jotai';

/** The sidebar node ID that is currently active (e.g. "system:active", "folder:5"). */
export const activeNodeIdAtom = atom<string>('system:active');
