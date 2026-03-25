/**
 * Canvas drawing primitives — shared by base and overlay draw layers.
 */

export const BADGE_H = 18;
export const BADGE_FONT = '600 10px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
export const BADGE_PAD_X = 5;
export const BADGE_RADIUS = 4;
export const NAME_FONT = '13px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
export const RATING_FONT = '10px sans-serif';
const BADGE_HIDDEN_TYPES = new Set(['jpg', 'jpeg', 'png', 'webp']);

const truncateCache = new Map<string, string>();

/** Draw a center-cropped image into the given rect (cover mode). */
export function drawImageCover(
  ctx: CanvasRenderingContext2D,
  img: ImageBitmap,
  dx: number, dy: number, dw: number, dh: number,
) {
  const imgAspect = img.width / img.height;
  const rectAspect = dw / dh;
  let sx: number, sy: number, sw: number, sh: number;

  if (imgAspect > rectAspect) {
    sh = img.height;
    sw = sh * rectAspect;
    sx = (img.width - sw) / 2;
    sy = 0;
  } else {
    sw = img.width;
    sh = sw / rectAspect;
    sx = 0;
    sy = (img.height - sh) / 2;
  }

  ctx.drawImage(img, sx, sy, sw, sh, dx, dy, dw, dh);
}

export function drawImageContain(
  ctx: CanvasRenderingContext2D,
  img: ImageBitmap,
  dx: number, dy: number, dw: number, dh: number,
) {
  const scale = Math.min(dw / img.width, dh / img.height);
  const sw = img.width * scale;
  const sh = img.height * scale;
  const ox = dx + (dw - sw) / 2;
  const oy = dy + (dh - sh) / 2;
  ctx.drawImage(img, 0, 0, img.width, img.height, ox, oy, sw, sh);
}

export function getContainRect(
  aspectRatio: number,
  dx: number,
  dy: number,
  dw: number,
  dh: number,
): { x: number; y: number; w: number; h: number } {
  const safe = Number.isFinite(aspectRatio) && aspectRatio > 0 ? aspectRatio : 1;
  const scale = Math.min(dw / safe, dh);
  const w = safe * scale;
  const h = scale;
  return {
    x: dx + (dw - w) / 2,
    y: dy + (dh - h) / 2,
    w,
    h,
  };
}

export function isHiddenBadgeType(ext: string): boolean {
  return BADGE_HIDDEN_TYPES.has(ext.toLowerCase());
}

/**
 * Draw a dark rounded-rect badge with white text.
 * Returns the total badge width (for positioning adjacent badges).
 * Assumes ctx.font is already set to BADGE_FONT by the caller.
 */
export function drawBadge(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number, y: number,
  align: 'left' | 'right' = 'right',
): number {
  const metrics = ctx.measureText(text);
  const w = metrics.width + BADGE_PAD_X * 2;
  const bx = align === 'right' ? x - w : x;

  ctx.fillStyle = 'rgba(0, 0, 0, 0.55)';
  ctx.beginPath();
  ctx.roundRect(bx, y, w, BADGE_H, BADGE_RADIUS);
  ctx.fill();

  ctx.fillStyle = 'rgba(255, 255, 255, 0.9)';
  ctx.textBaseline = 'middle';
  ctx.fillText(text, bx + BADGE_PAD_X, y + BADGE_H / 2);

  return w;
}

/** Truncate text with ellipsis, cached by (text, maxWidth). Binary search. */
export function truncateText(
  ctx: CanvasRenderingContext2D,
  text: string,
  maxWidth: number,
): string {
  const key = `${text}|${maxWidth}`;
  const cached = truncateCache.get(key);
  if (cached !== undefined) return cached;

  if (ctx.measureText(text).width <= maxWidth) {
    truncateCache.set(key, text);
    return text;
  }

  // Binary search for longest prefix that fits with ellipsis
  let lo = 0;
  let hi = text.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >>> 1;
    if (ctx.measureText(text.slice(0, mid) + '…').width <= maxWidth) {
      lo = mid;
    } else {
      hi = mid - 1;
    }
  }
  const result = lo > 0 ? text.slice(0, lo) + '…' : '…';
  truncateCache.set(key, result);

  if (truncateCache.size > 5000) {
    const firstKey = truncateCache.keys().next().value;
    if (firstKey) truncateCache.delete(firstKey);
  }

  return result;
}

/** Format milliseconds as m:ss. */
export function formatDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  const sec = s % 60;
  return `${m}:${sec.toString().padStart(2, '0')}`;
}

const MIME_TO_EXT: Record<string, string> = {
  'image/jpeg': 'JPG', 'image/png': 'PNG', 'image/gif': 'GIF',
  'image/webp': 'WEBP', 'image/svg+xml': 'SVG', 'image/bmp': 'BMP',
  'image/tiff': 'TIFF', 'image/avif': 'AVIF', 'image/heic': 'HEIC',
  'video/mp4': 'MP4', 'video/webm': 'WEBM', 'video/quicktime': 'MOV',
  'video/x-matroska': 'MKV', 'video/avi': 'AVI',
  'audio/mpeg': 'MP3', 'audio/wav': 'WAV', 'audio/flac': 'FLAC',
};

/** Map MIME type to file extension for badge display. */
export function mimeToExt(mime: string): string {
  return MIME_TO_EXT[mime] ?? mime.split('/')[1]?.toUpperCase() ?? '';
}
