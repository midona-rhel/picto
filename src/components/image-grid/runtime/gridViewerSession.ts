import type { MasonryImageItem } from '../shared';

// ---------------------------------------------------------------------------
// ViewerSession — centralized navigation state for detail/peek/window viewers
// ---------------------------------------------------------------------------

export interface ViewerSession {
  currentIndex: number;
  currentHash: string;
}

/**
 * Create a new session anchored at the given hash.
 * Returns index 0 if hash is not found in images.
 */
export function createSession(
  images: MasonryImageItem[],
  hash: string,
): ViewerSession {
  const idx = images.findIndex(i => i.hash === hash);
  const index = idx >= 0 ? idx : 0;
  return {
    currentIndex: index,
    currentHash: images[index]?.hash ?? hash,
  };
}

/**
 * Navigate by delta (e.g. -1 for prev, +1 for next). Clamps to bounds.
 */
export function navigateSession(
  session: ViewerSession,
  images: MasonryImageItem[],
  delta: number,
): ViewerSession {
  if (images.length === 0) return session;
  const next = Math.max(0, Math.min(images.length - 1, session.currentIndex + delta));
  if (next === session.currentIndex) return session;
  return {
    currentIndex: next,
    currentHash: images[next].hash,
  };
}

/**
 * Rebase the session onto a new image list.
 * Finds currentHash in the new list to maintain position.
 * Returns null if the current hash is gone (caller should close the viewer).
 */
export function rebaseSession(
  session: ViewerSession,
  newImages: MasonryImageItem[],
): ViewerSession | null {
  if (newImages.length === 0) return null;
  const idx = newImages.findIndex(i => i.hash === session.currentHash);
  if (idx >= 0) {
    if (idx === session.currentIndex) return session; // no change
    return { currentIndex: idx, currentHash: session.currentHash };
  }
  // Hash gone — return null so caller can decide (close viewer or clamp)
  return null;
}

/**
 * Clamp session index to the given max (images.length - 1).
 * Used after inbox removal to keep index in bounds.
 */
export function clampSession(
  session: ViewerSession,
  images: MasonryImageItem[],
): ViewerSession {
  if (images.length === 0) return session;
  const clamped = Math.min(session.currentIndex, images.length - 1);
  return {
    currentIndex: clamped,
    currentHash: images[clamped].hash,
  };
}
