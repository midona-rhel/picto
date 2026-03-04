export const isEditableTarget = (target: EventTarget | null): boolean => {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tagName = target.tagName;
  if (tagName === 'INPUT' || tagName === 'TEXTAREA' || tagName === 'SELECT') return true;
  return target.closest('[contenteditable="true"]') !== null;
};

const MEDIA_MIME_BY_EXTENSION: Record<string, string> = {
  webp: 'image/webp',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  png: 'image/png',
  gif: 'image/gif',
  bmp: 'image/bmp',
  webm: 'video/webm',
  mp4: 'video/mp4',
  mov: 'video/quicktime',
  mkv: 'video/x-matroska',
  avi: 'video/x-msvideo',
};

export const mediaMimeForFilename = (filename: string) => {
  const ext = filename.split('.').pop()?.toLowerCase() ?? '';
  return MEDIA_MIME_BY_EXTENSION[ext] || 'image/jpeg';
};

export const isVideoMime = (mime: string) => mime.startsWith('video/');

export interface ImageItem {
  entity_id?: number;
  is_collection?: boolean;
  collection_item_count?: number | null;
  hash: string;
  name: string | null;
  mime: string;
  size: number;
  width: number | null;
  height: number | null;
  duration_ms: number | null;
  num_frames: number | null;
  has_audio: boolean;
  status: string;
  rating: number | null;
  view_count: number;
  source_urls: string[] | null;
  imported_at: string;
  has_thumbnail: boolean;
  blurhash?: string | null;
  tags?: string[];
  dominant_colors?: { hex: string; l: number; a: number; b: number }[] | null;
  notes?: Record<string, string> | null;
}

// Extended type for Masonic grid with computed aspect ratio
export interface MasonryImageItem extends ImageItem {
  aspectRatio: number;
}

function sanitizeDimension(value: number | null | undefined, fallback: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value) || value <= 1) return fallback;
  return value;
}

function sanitizeAspectRatio(raw: number): number {
  if (!Number.isFinite(raw) || raw <= 0) return 1.5;
  // Guard against pathological ratios that destabilize waterfall layout.
  return Math.min(8, Math.max(0.125, raw));
}

/** Convert a FileInfo from the backend into a MasonryImageItem */
export function toMasonryItem(file: ImageItem): MasonryImageItem {
  const w = sanitizeDimension(file.width, 300);
  const h = sanitizeDimension(file.height, 200);
  return { ...file, aspectRatio: sanitizeAspectRatio(w / h) };
}
