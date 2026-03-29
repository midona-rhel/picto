/**
 * Media URL helpers — single source of truth for thumbnail and file URLs.
 */

const MIME_EXT: Record<string, string> = {
  'image/jpeg': 'jpg', 'image/png': 'png', 'image/gif': 'gif', 'image/webp': 'webp',
  'image/bmp': 'bmp', 'image/tiff': 'tiff', 'image/avif': 'avif', 'image/heic': 'heif',
  'image/svg+xml': 'svg', 'video/mp4': 'mp4', 'video/webm': 'webm',
  'video/quicktime': 'mov', 'video/x-matroska': 'mkv',
};

export function mimeToMediaExt(mime: string): string {
  return MIME_EXT[mime] ?? 'bin';
}

export function mediaThumbnailUrl(hash: string): string {
  return `media://localhost/thumb/${hash}.jpg`;
}

export function mediaFileUrl(hash: string, mime: string): string {
  return `media://localhost/file/${hash}.${mimeToMediaExt(mime)}`;
}
