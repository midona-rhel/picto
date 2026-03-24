/**
 * Runtime state — backend event reconciliation.
 *
 * Receives state-change events from the backend and tracks what
 * needs to be refreshed. Appliers consume these to update grid,
 * sidebar, and metadata atoms.
 */

import { atom } from 'jotai';

export interface RuntimeTask {
  task_id: string;
  kind: string;
  family: string;
  label: string;
  progress: number | null;
  status: string;
  detail: string | null;
}

// ── Backend tasks ──────────────────────────────────────────────

export const runtimeTasksAtom = atom<Map<string, RuntimeTask>>(new Map());

// ── State-change reconciliation ────────────────────────────────

/** Last processed event sequence number. */
export const lastEventSeqAtom = atom(0);

/** Grid scopes from the last event that carried entity hashes.
 *  Used to gate eager insertion — only insert tiles when viewing
 *  a matching scope. */
export const lastInsertionScopesAtom = atom<string[] | null>(null);

/** Origin label from the last state-change event. */
export const lastChangeOriginAtom = atom<string | null>(null);

/** Sidebar counts from the last event (if included). */
export const runtimeSidebarCountsAtom = atom<{
  active: number;
  inbox: number;
  trash: number;
} | null>(null);
