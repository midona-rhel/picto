import { useEffect } from 'react';

// Module-level set of URLs confirmed decoded (decode() resolved).
// Survives across component mounts — cleared only on page reload.
const preloadedUrls = new Set<string>();

interface DecodeTask {
  url: string;
  heavy: boolean;
  cancelled: boolean;
  image: HTMLImageElement | null;
  onReady: () => void;
  priority: 'high' | 'normal';
}

const MAX_ACTIVE_DECODES = 2;
const MAX_ACTIVE_HEAVY_DECODES = 1;
let activeDecodes = 0;
let activeHeavyDecodes = 0;
const decodeQueue: DecodeTask[] = [];

function isHeavyDecodeUrl(url: string): boolean {
  const lower = url.toLowerCase();
  return lower.endsWith('.webp') || lower.endsWith('.avif');
}

function finishTask(task: DecodeTask): void {
  activeDecodes = Math.max(0, activeDecodes - 1);
  if (task.heavy) {
    activeHeavyDecodes = Math.max(0, activeHeavyDecodes - 1);
  }
  pumpDecodeQueue();
}

function startTask(task: DecodeTask): void {
  activeDecodes++;
  if (task.heavy) activeHeavyDecodes++;

  const img = new Image();
  task.image = img;
  img.src = task.url;
  let settled = false;
  let timeoutId: ReturnType<typeof setTimeout> | null = null;

  const settle = (callReady: boolean): void => {
    if (settled) return;
    settled = true;
    if (timeoutId) {
      clearTimeout(timeoutId);
      timeoutId = null;
    }
    if (callReady && !task.cancelled) task.onReady();
    if (task.image) {
      task.image.onload = null;
      task.image.onerror = null;
      task.image.src = '';
      task.image = null;
    }
    finishTask(task);
  };

  // Safety net: never let a decode task pin an active slot forever.
  timeoutId = setTimeout(() => {
    settle(img.naturalWidth > 0);
  }, 10000);

  img.decode()
    .then(() => {
      if (!task.cancelled) {
        markAsPreloaded(task.url);
        settle(true);
      } else {
        settle(false);
      }
    })
    .catch(() => {
      // decode() can reject for protocol/browser reasons even when the image
      // eventually loads fine. Wait for onload before surfacing the URL to
      // avoid partial top-to-bottom paints in the detail view.
      if (task.cancelled) {
        settle(false);
        return;
      }
      if (img.complete && img.naturalWidth > 0) {
        // Try decode() once data is fully loaded. Only mark preloaded if this
        // succeeds; onload alone is not a reliable "fully decoded" signal.
        img.decode()
          .then(() => {
            if (!task.cancelled) markAsPreloaded(task.url);
            settle(!task.cancelled);
          })
          .catch(() => {
            settle(!task.cancelled);
          });
        return;
      }
      const finalize = () => {
        if (task.cancelled || img.naturalWidth <= 0) {
          settle(false);
          return;
        }
        img.decode()
          .then(() => {
            if (!task.cancelled) markAsPreloaded(task.url);
            settle(!task.cancelled);
          })
          .catch(() => {
            settle(!task.cancelled);
          });
      };
      img.onload = finalize;
      img.onerror = finalize;
    })
    .finally(() => {
      // No-op: completion is handled by settle() to avoid double-finish.
    });
}

function pumpDecodeQueue(): void {
  if (activeDecodes >= MAX_ACTIVE_DECODES || decodeQueue.length === 0) return;

  let nextIdx = -1;
  for (let i = 0; i < decodeQueue.length; i++) {
    const task = decodeQueue[i];
    if (task.cancelled) continue;
    if (task.heavy && activeHeavyDecodes >= MAX_ACTIVE_HEAVY_DECODES) continue;
    nextIdx = i;
    break;
  }

  if (nextIdx < 0) return;
  const [nextTask] = decodeQueue.splice(nextIdx, 1);
  if (!nextTask || nextTask.cancelled) {
    pumpDecodeQueue();
    return;
  }
  startTask(nextTask);
  pumpDecodeQueue();
}

/** Check if a URL's image data is already decoded in browser cache. */
export function isImagePreloaded(url: string): boolean {
  return preloadedUrls.has(url);
}

/** Mark a URL as confirmed decoded in browser cache. */
export function markAsPreloaded(url: string): void {
  preloadedUrls.add(url);
}

export function queueImageDecode(
  imageUrl: string,
  onReady: (url: string) => void,
  priority: 'high' | 'normal' = 'normal',
): () => void {
  if (isImagePreloaded(imageUrl)) {
    onReady(imageUrl);
    return () => {};
  }

  const task: DecodeTask = {
    url: imageUrl,
    heavy: isHeavyDecodeUrl(imageUrl),
    cancelled: false,
    image: null,
    onReady: () => onReady(imageUrl),
    priority,
  };

  if (priority === 'high') {
    decodeQueue.unshift(task);
  } else {
    decodeQueue.push(task);
  }
  pumpDecodeQueue();

  return () => {
    task.cancelled = true;
    // Don't force-abort active image decode here; allow settle()/timeout path
    // to release active slots deterministically.
    const idx = decodeQueue.indexOf(task);
    if (idx >= 0) decodeQueue.splice(idx, 1);
  };
}

/**
 * Preloads an image via decode() — decodes off the main thread.
 * Calls `onReady(url)` once the decoded bitmap is in browser cache.
 * The DOM <img> should only set this URL as src AFTER onReady fires,
 * so the render is instant with zero main-thread decode work.
 */
export function useImagePreloader(
  imageUrl: string | null,
  isVideo: boolean,
  onReady: (url: string) => void,
): void {
  useEffect(() => {
    if (!imageUrl || isVideo) return;
    return queueImageDecode(imageUrl, onReady, 'high');
  }, [imageUrl, isVideo, onReady]);
}
