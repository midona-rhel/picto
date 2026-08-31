/**
 * Media URL helpers — single source of truth for thumbnail and file URLs.
 */

const MIME_EXT: Record<string, string> = {
  'image/jpeg': 'jpg', 'image/png': 'png', 'image/gif': 'gif', 'image/webp': 'webp',
  'image/bmp': 'bmp', 'image/tiff': 'tiff', 'image/avif': 'avif', 'image/heic': 'heif',
  'image/jxl': 'jxl',
  'image/svg+xml': 'svg', 'video/mp4': 'mp4', 'video/webm': 'webm',
  'video/quicktime': 'mov', 'video/x-matroska': 'mkv',
  'audio/aac': 'aac', 'audio/flac': 'flac', 'audio/mp4': 'm4a',
  'audio/mpeg': 'mp3', 'audio/ogg': 'ogg', 'audio/opus': 'opus',
  'audio/wav': 'wav', 'audio/x-wav': 'wav', 'audio/x-ms-wma': 'wma',
  'audio/x-matroska': 'mka', 'audio/wavpack': 'wv', 'audio/x-tta': 'tta',
  'application/pdf': 'pdf', 'application/epub+zip': 'epub',
  'application/vnd.openxmlformats-officedocument.wordprocessingml.document': 'docx',
  'application/vnd.openxmlformats-officedocument.presentationml.presentation': 'pptx',
  'application/vnd.comicbook+zip': 'cbz', 'application/x-cbz': 'cbz',
  'image/vnd.djvu': 'djvu', 'image/x-djvu': 'djvu',
  'text/plain': 'txt', 'text/markdown': 'md', 'application/json': 'json',
  'application/rtf': 'rtf', 'text/rtf': 'rtf',
  'application/x-shockwave-flash': 'swf',
  'font/ttf': 'ttf', 'font/collection': 'ttc', 'font/otf': 'otf', 'font/woff': 'woff',
};

export function mimeToMediaExt(mime: string): string {
  return MIME_EXT[mime] ?? 'bin';
}

export function mediaThumbnailUrl(hash: string): string {
  return `media://localhost/thumb/${hash}.jpg`;
}

export function libraryCoverUrl(libraryPath: string, version?: string | null): string {
  const base = `media://localhost/library-cover/cover?library=${encodeURIComponent(libraryPath)}`;
  return version ? `${base}&v=${encodeURIComponent(version)}` : base;
}

export function mediaFileUrl(hash: string, mime: string): string {
  return `media://localhost/file/${hash}.${mimeToMediaExt(mime)}`;
}
