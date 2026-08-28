/**
 * Viewer state — inline media view overlay.
 *
 * The viewer reads grid items from gridItemsAtom. Opening captures the
 * current item ID + index. Navigation updates the session.
 * Closing restores grid scroll to the exit item.
 */

import { atom } from 'jotai';
import type { CanonicalEntityGridItem } from '../shared/types/canonical';
import { gridItemsAtom } from './grid';

export interface ViewerSession {
  currentIndex: number;
  currentItemId: number;
}

/** null = viewer closed. */
export const viewerSessionAtom = atom<ViewerSession | null>(null);

export const viewerOpenAtom = atom((get) => get(viewerSessionAtom) != null);

/** null = quicklook closed. */
export const quickLookSessionAtom = atom<ViewerSession | null>(null);

/** True while an outgoing viewer/editor is being replaced by another workspace scope. */
export const viewerExitTransitionAtom = atom(false);

// ── Viewer ↔ toolbar communication atoms ──

export interface ViewerDisplayState {
  currentIndex: number;
  total: number;
  zoomPercent?: number;
}

export interface ViewerZoomControls {
  fitToWindow: () => void;
  fitActual: () => void;
  zoomIn: () => void;
  zoomOut: () => void;
  setZoomScale: (scale: number) => void;
  subscribeZoomScale: (listener: (scale: number) => void) => () => void;
}

export interface ViewerDisplayControls {
  close: () => void;
  navigate?: (delta: number) => void;
  zoom?: ViewerZoomControls;
  edit?: () => void;
  backLabel?: string;
}

/** Written by MediaView, read by GridToolbar. */
export const viewerDisplayStateAtom = atom<ViewerDisplayState | null>(null);
/** Written by MediaView, read by GridToolbar. */
export const viewerDisplayControlsAtom = atom<ViewerDisplayControls | null>(null);

/**
 * The session is anchored by item ID — the stored index is only a hint.
 * Items can be inserted ahead of the current position while a run ingests;
 * resolving by index alone would silently shift which image is shown.
 */
export function resolveViewerIndex(
  session: ViewerSession,
  items: CanonicalEntityGridItem[],
): number {
  if (items[session.currentIndex]?.root_id === session.currentItemId) {
    return session.currentIndex;
  }
  const byId = items.findIndex((item) => item.root_id === session.currentItemId);
  if (byId >= 0) return byId;
  // Current entity vanished (deleted/moved out of scope): stay near position.
  return Math.min(session.currentIndex, Math.max(items.length - 1, 0));
}

export const viewerCurrentItemAtom = atom<CanonicalEntityGridItem | null>((get) => {
  const session = get(viewerSessionAtom);
  if (!session) return null;
  const items = get(gridItemsAtom);
  return items[resolveViewerIndex(session, items)] ?? null;
});

/** Create a session from items + target item ID. */
export function createViewerSession(
  items: CanonicalEntityGridItem[],
  itemId: number,
): ViewerSession | null {
  const index = items.findIndex((item) => item.root_id === itemId);
  if (index < 0) return null;
  return { currentIndex: index, currentItemId: itemId };
}

/** Navigate by delta from the ID-anchored position, clamped to bounds. */
export function navigateViewerSession(
  session: ViewerSession,
  items: CanonicalEntityGridItem[],
  delta: number,
): ViewerSession | null {
  const nextIndex = resolveViewerIndex(session, items) + delta;
  if (nextIndex < 0 || nextIndex >= items.length) return null;
  return { currentIndex: nextIndex, currentItemId: items[nextIndex].root_id };
}
