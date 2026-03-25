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

export function mimeToMediaExtension(mime: string): string {
  return MIME_TO_EXT[mime] ?? 'bin';
}

export function mediaFileUrl(hash: string, mime: string): string {
  return `media://localhost/file/${hash}.${mimeToMediaExtension(mime)}`;
}

export function mediaThumbnailUrl(hash: string): string {
  return `media://localhost/thumb/${hash}.jpg`;
}
