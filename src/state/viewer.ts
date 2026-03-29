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

export const viewerCurrentItemAtom = atom<CanonicalEntityGridItem | null>((get) => {
  const session = get(viewerSessionAtom);
  if (!session) return null;
  const items = get(gridItemsAtom);
  return items[session.currentIndex] ?? null;
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

/** Navigate by delta, clamped to bounds. Returns null if out of range. */
export function navigateViewerSession(
  session: ViewerSession,
  items: CanonicalEntityGridItem[],
  delta: number,
): ViewerSession | null {
  const nextIndex = session.currentIndex + delta;
  if (nextIndex < 0 || nextIndex >= items.length) return null;
  return { currentIndex: nextIndex, currentHash: items[nextIndex].entity_hash };
}
