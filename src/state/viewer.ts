/**
 * Viewer state — inline media view overlay.
 *
 * The viewer reads grid items from gridItemsAtom. Opening captures the
 * current entity hash + index. Navigation updates the session.
 * Closing restores grid scroll to the exit hash.
 */

import { atom } from 'jotai';
import type { CanonicalEntityGridItem } from '../shared/types/canonical';
import { gridItemsAtom } from './grid';

export interface ViewerSession {
  currentIndex: number;
  currentHash: string;
}

/** null = viewer closed. */
export const viewerSessionAtom = atom<ViewerSession | null>(null);

export const viewerOpenAtom = atom((get) => get(viewerSessionAtom) != null);

/** null = quicklook closed. */
export const quickLookSessionAtom = atom<ViewerSession | null>(null);

// ── Viewer ↔ toolbar communication atoms ──

export interface ViewerDisplayState {
  currentIndex: number;
  total: number;
  zoomPercent: number;
}

export interface ViewerDisplayControls {
  close: () => void;
  navigate: (delta: number) => void;
  fitToWindow: () => void;
  fitActual: () => void;
  zoomIn: () => void;
  zoomOut: () => void;
  setZoomScale: (scale: number) => void;
}

/** Written by MediaView, read by GridToolbar. */
export const viewerDisplayStateAtom = atom<ViewerDisplayState | null>(null);
/** Written by MediaView, read by GridToolbar. */
export const viewerDisplayControlsAtom = atom<ViewerDisplayControls | null>(null);

/**
 * The session is anchored by entity hash — the stored index is only a hint.
 * Items can be inserted ahead of the current position while a run ingests;
 * resolving by index alone would silently shift which image is shown.
 */
export function resolveViewerIndex(
  session: ViewerSession,
  items: CanonicalEntityGridItem[],
): number {
  if (items[session.currentIndex]?.entity_hash === session.currentHash) {
    return session.currentIndex;
  }
  const byHash = items.findIndex((item) => item.entity_hash === session.currentHash);
  if (byHash >= 0) return byHash;
  // Current entity vanished (deleted/moved out of scope): stay near position.
  return Math.min(session.currentIndex, Math.max(items.length - 1, 0));
}

export const viewerCurrentItemAtom = atom<CanonicalEntityGridItem | null>((get) => {
  const session = get(viewerSessionAtom);
  if (!session) return null;
  const items = get(gridItemsAtom);
  return items[resolveViewerIndex(session, items)] ?? null;
});

/** Create a session from items + target hash. */
export function createViewerSession(
  items: CanonicalEntityGridItem[],
  hash: string,
): ViewerSession | null {
  const index = items.findIndex((item) => item.entity_hash === hash);
  if (index < 0) return null;
  return { currentIndex: index, currentHash: hash };
}

/** Navigate by delta from the hash-anchored position, clamped to bounds. */
export function navigateViewerSession(
  session: ViewerSession,
  items: CanonicalEntityGridItem[],
  delta: number,
): ViewerSession | null {
  const nextIndex = resolveViewerIndex(session, items) + delta;
  if (nextIndex < 0 || nextIndex >= items.length) return null;
  return { currentIndex: nextIndex, currentHash: items[nextIndex].entity_hash };
}
