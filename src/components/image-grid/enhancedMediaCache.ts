/**
 * Media cache backed by the `media://` custom protocol.
 *
 * All media is served via a desktop URI scheme protocol that maps hashes
 * directly to on-disk blobs. The browser engine handles caching natively
 * via `Cache-Control: immutable` headers, so this module just builds
 * deterministic URLs.
 *
 * Replaces the old IPC-based cache that fetched Vec<u8> over JSON,
 * converted to Blob URLs, and managed an LRU eviction policy.
 */

import { mediaFileUrl, mediaThumbnailUrl } from '../../lib/mediaUrl';
import { ImageItem } from './shared';

export type MediaVariant = 'thumb64' | 'thumb512' | 'full';

// Lightweight hash → MIME map so getCachedMediaUrl() can construct
// full-file protocol URLs without needing the full ImageItem.
// Populated whenever preloadMediaUrl() is called.
const mimeMap = new Map<string, string>();

function urlForVariant(image: ImageItem, variant: MediaVariant): string {
  if (variant === 'full') {
    return mediaFileUrl(image.hash, image.mime);
  }
  return mediaThumbnailUrl(image.hash);
}

/**
 * Synchronously return a protocol URL if one can be constructed.
 *
 * For thumbnails this always works (MIME is always `image/jpeg`).
 * For full files, works only after preloadMediaUrl() has recorded the MIME.
 */
export const getCachedMediaUrl = (imageHash: string, variant: MediaVariant): string => {
  if (variant === 'full') {
    const mime = mimeMap.get(imageHash);
    if (!mime) return '';
    return mediaFileUrl(imageHash, mime);
  }
  return mediaThumbnailUrl(imageHash);
};

export const hasCachedMediaUrl = (imageHash: string, variant: MediaVariant): boolean => {
  if (variant === 'full') return mimeMap.has(imageHash);
  return true;
};

/**
 * Return the protocol URL for an image variant.
 *
 * With the protocol approach this is effectively synchronous — we just
 * build a URL string. The actual network fetch happens when the browser
 * renders the element. The async signature is kept for API compatibility.
 */
export const preloadMediaUrl = async (image: ImageItem, variant: MediaVariant): Promise<string> => {
  // Record MIME so future getCachedMediaUrl('full') calls work
  mimeMap.set(image.hash, image.mime);
  return urlForVariant(image, variant);
};

export const batchPreloadMediaUrls = async (
  images: ImageItem[],
  _variant: MediaVariant,
  _priority?: 'high' | 'low',
): Promise<void> => {
  // Record MIMEs; actual loading is on-demand by the browser
  for (const image of images) {
    mimeMap.set(image.hash, image.mime);
  }
};

export const getCacheStats = () => ({
  thumb64: { size: 0, totalSize: 0, memoryPressure: 0 },
  thumb512: { size: 0, totalSize: 0, memoryPressure: 0 },
  full: { size: mimeMap.size, totalSize: 0, memoryPressure: 0 },
});

export const decodeImageUrl = (url: string): Promise<void> => {
  return new Promise((resolve) => {
    const img = new Image();
    let settled = false;
    const done = () => {
      if (settled) return;
      settled = true;
      resolve();
    };
    img.onload = done;
    img.onerror = done;
    img.src = url;
    if ('decode' in img) {
      img.decode().then(done).catch(done);
    }
  });
};

export const cleanupMediaCache = () => {
  mimeMap.clear();
};
