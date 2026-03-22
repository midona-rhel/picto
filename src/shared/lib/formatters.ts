export function formatFileSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export function formatDuration(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) {
    return `${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;
  }
  return `${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;
}

export function formatDateTime(isoString: string): string {
  const d = new Date(isoString);
  if (isNaN(d.getTime())) return isoString;
  return d.toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

const MIME_EXT_MAP: Record<string, string> = {
  jpeg: 'JPG', png: 'PNG', gif: 'GIF', webp: 'WEBP', 'svg+xml': 'SVG',
  mp4: 'MP4', webm: 'WEBM', quicktime: 'MOV', 'x-matroska': 'MKV',
  bmp: 'BMP', tiff: 'TIFF', avif: 'AVIF', heic: 'HEIC',
  flac: 'FLAC', mpeg: 'MP3', ogg: 'OGG', wav: 'WAV',
};

export function getFileExtension(name: string | null, mime: string | null): string {
  // Prefer MIME type (detected from file bytes) over filename extension
  // because subscription downloads often have incorrect or missing extensions.
  if (mime) {
    const sub = mime.split('/').pop()?.toLowerCase();
    if (sub) return MIME_EXT_MAP[sub] ?? sub.toUpperCase();
  }
  if (name) {
    const dotIdx = name.lastIndexOf('.');
    if (dotIdx > 0) return name.slice(dotIdx + 1).toUpperCase();
  }
  return 'Unknown';
}
