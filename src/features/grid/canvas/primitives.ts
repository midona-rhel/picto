/**
 * Canvas drawing primitives — shared by base and overlay draw layers.
 * Matches legacy canvasGridPrimitives.ts rendering exactly.
 */

export const BADGE_H = 18;
export const BADGE_FONT = '600 10px -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif';
export const NAME_FONT = '400 12px -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif';
export const INFO_FONT = '400 11px -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif';

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
    // Image is wider — crop sides
    sh = img.height;
    sw = sh * rectAspect;
    sx = (img.width - sw) / 2;
    sy = 0;
  } else {
    // Image is taller — crop top/bottom
    sw = img.width;
    sh = sw / rectAspect;
    sx = 0;
    sy = (img.height - sh) / 2;
  }

  ctx.drawImage(img, sx, sy, sw, sh, dx, dy, dw, dh);
}

/** Draw a dark rounded-rect badge with white text. */
export function drawBadge(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number, y: number,
  align: 'left' | 'right' = 'right',
) {
  ctx.font = BADGE_FONT;
  const metrics = ctx.measureText(text);
  const padX = 5;
  const w = metrics.width + padX * 2;
  const h = BADGE_H;
  const bx = align === 'right' ? x - w : x;
  const by = y;
  const r = 4;

  // Background
  ctx.fillStyle = 'rgba(0, 0, 0, 0.55)';
  ctx.beginPath();
  ctx.roundRect(bx, by, w, h, r);
  ctx.fill();

  // Text
  ctx.fillStyle = 'rgba(255, 255, 255, 0.9)';
  ctx.textBaseline = 'middle';
  ctx.fillText(text, bx + padX, by + h / 2);
}

/** Truncate text with ellipsis, cached by (text, maxWidth). */
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

  let truncated = text;
  while (truncated.length > 0 && ctx.measureText(truncated + '…').width > maxWidth) {
    truncated = truncated.slice(0, -1);
  }
  const result = truncated + '…';
  truncateCache.set(key, result);

  // Prevent cache from growing unbounded
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

/** Map MIME type to file extension for badge display. */
export function mimeToExt(mime: string): string {
  const map: Record<string, string> = {
    'image/jpeg': 'JPG', 'image/png': 'PNG', 'image/gif': 'GIF',
    'image/webp': 'WEBP', 'image/svg+xml': 'SVG', 'image/bmp': 'BMP',
    'image/tiff': 'TIFF', 'image/avif': 'AVIF', 'image/heic': 'HEIC',
    'video/mp4': 'MP4', 'video/webm': 'WEBM', 'video/quicktime': 'MOV',
    'video/x-matroska': 'MKV', 'video/avi': 'AVI',
    'audio/mpeg': 'MP3', 'audio/wav': 'WAV', 'audio/flac': 'FLAC',
  };
  return map[mime] ?? mime.split('/')[1]?.toUpperCase() ?? '';
}
