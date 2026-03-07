/**
 * URL builders for the `media://` custom protocol.
 *
 * All media (images, video, audio, PDFs) is served via this protocol,
 * which maps directly to content-addressed blobs on disk. The browser
 * handles caching via `Cache-Control: immutable`.
 *
 * Strict format: `media://localhost/file/<hash>.<ext>` — MIME derived from extension.
 */

const MIME_TO_EXT: Record<string, string> = {
  'image/jpeg': 'jpg',
  'image/png': 'png',
  'image/gif': 'gif',
  'image/webp': 'webp',
  'image/bmp': 'bmp',
  'image/tiff': 'tiff',
  'image/svg+xml': 'svg',
  'image/avif': 'avif',
  'image/heif': 'heif',
  'image/heic': 'heif',
  'image/jxl': 'jxl',
  'image/x-icon': 'ico',
  'image/vnd.adobe.photoshop': 'psd',
  'video/mp4': 'mp4',
  'video/webm': 'webm',
  'video/x-matroska': 'mkv',
  'video/quicktime': 'mov',
  'video/x-flv': 'flv',
  'video/x-msvideo': 'avi',
  'audio/flac': 'flac',
  'audio/x-wav': 'wav',
  'audio/wav': 'wav',
  'application/pdf': 'pdf',
  'application/epub+zip': 'epub',
};

/** Convert a MIME type to a file extension. */
export function mimeToExtension(mime: string): string {
  return MIME_TO_EXT[mime] ?? 'bin';
}

/** URL for an original file. Extension derived from MIME for Content-Type. */
export function mediaFileUrl(hash: string, mime: string): string {
  const ext = mimeToExtension(mime);
  return `media://localhost/file/${hash}.${ext}`;
}

/** Cache-busting nonces for regenerated thumbnails. When a thumbnail is
 *  regenerated, add its hash here so the next URL includes a nonce that
 *  bypasses the browser's `immutable` HTTP cache. */
const thumbnailBustNonces = new Map<string, number>();

/** Mark hashes as needing cache-busted thumbnail URLs.
 *  Call after regenerating thumbnails. */
export function bustThumbnailCache(hashes: string[]): void {
  const now = Date.now();
  for (const h of hashes) thumbnailBustNonces.set(h, now);
}

/** URL for a thumbnail. Includes a cache-busting nonce if the thumbnail
 *  was recently regenerated. */
export function mediaThumbnailUrl(hash: string): string {
  const nonce = thumbnailBustNonces.get(hash);
  if (nonce !== undefined) {
    thumbnailBustNonces.delete(hash);
    return `media://localhost/thumb/${hash}.jpg?v=${nonce}`;
  }
  return `media://localhost/thumb/${hash}.jpg`;
}
