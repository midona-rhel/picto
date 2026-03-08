import { useEffect } from 'react';
import { enqueueMediaQosTask, type MediaQosLane } from './mediaQosScheduler';

// Module-level set of URLs confirmed decoded (decode() resolved).
// Survives across component mounts — cleared only on page reload.
const preloadedUrls = new Set<string>();

function isHeavyDecodeUrl(url: string): boolean {
  const lower = url.toLowerCase();
  return lower.endsWith('.webp') || lower.endsWith('.avif');
}

function decodeImageUrlWithSignal(url: string, signal: AbortSignal): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    const img = new Image();
    let settled = false;
    let timeoutId: ReturnType<typeof setTimeout> | null = null;

    const cleanup = () => {
      if (timeoutId) {
        clearTimeout(timeoutId);
        timeoutId = null;
      }
      img.onload = null;
      img.onerror = null;
      img.src = '';
      signal.removeEventListener('abort', onAbort);
    };

    const finish = (ok: boolean) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(ok && !signal.aborted);
    };

    const onAbort = () => {
      finish(false);
    };

    signal.addEventListener('abort', onAbort, { once: true });
    img.src = url;

    // Safety net: don't let a stuck decode consume a scheduler slot forever.
    timeoutId = setTimeout(() => {
      finish(img.naturalWidth > 0);
    }, 10000);

    img.decode()
      .then(() => {
        finish(true);
      })
      .catch(() => {
        // decode() can reject while image still completes correctly.
        if (signal.aborted) {
          finish(false);
          return;
        }

        if (img.complete && img.naturalWidth > 0) {
          img.decode()
            .then(() => finish(true))
            .catch(() => finish(true));
          return;
        }

        const finalize = () => {
          if (signal.aborted || img.naturalWidth <= 0) {
            finish(false);
            return;
          }
          img.decode()
            .then(() => finish(true))
            .catch(() => finish(true));
        };

        img.onload = finalize;
        img.onerror = finalize;
      });
  });
}

/** Check if a URL's image data is already decoded in browser cache. */
export function isImagePreloaded(url: string): boolean {
  return preloadedUrls.has(url);
}

/** Mark a URL as confirmed decoded in browser cache. */
export function markAsPreloaded(url: string): void {
  preloadedUrls.add(url);
}

function laneForPriority(priority: 'high' | 'normal'): MediaQosLane {
  return priority === 'high' ? 'critical' : 'prefetch';
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

  const lane = laneForPriority(priority);
  const handle = enqueueMediaQosTask({
    lane,
    priority: priority === 'high' ? 0 : 50,
    heavy: isHeavyDecodeUrl(imageUrl),
    run: async (signal) => {
      const decoded = await decodeImageUrlWithSignal(imageUrl, signal);
      if (!decoded || signal.aborted) return;
      markAsPreloaded(imageUrl);
      onReady(imageUrl);
    },
  });

  return () => {
    handle.cancel();
  };
}

/**
 * Preloads an image via decode() — decoded via shared QoS scheduler.
 * Calls `onReady(url)` once the decoded bitmap is in browser cache.
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
