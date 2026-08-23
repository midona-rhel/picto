import { GRID_BADGE_FONT, GRID_INFO_FONT, GRID_NAME_FONT, GRID_RATING_FONT, GRID_TILE_RADIUS } from '../gridAppearance';

export const BADGE_H = 18;
export const BADGE_FONT = GRID_BADGE_FONT;
export const BADGE_PAD_X = 4;
export const BADGE_RADIUS = GRID_TILE_RADIUS;
export const NAME_FONT = GRID_NAME_FONT;
export const INFO_FONT = GRID_INFO_FONT;
export const RATING_FONT = GRID_RATING_FONT;

const BADGE_HIDDEN_TYPES = new Set(['jpg', 'jpeg', 'png', 'webp']);
const truncateCache = new Map<string, string>();

export function isHiddenBadgeType(ext: string): boolean {
  return BADGE_HIDDEN_TYPES.has(ext.toLowerCase());
}

const MIME_TO_EXT: Record<string, string> = {
  jpeg: 'jpg', png: 'png', gif: 'gif', webp: 'webp', 'svg+xml': 'svg',
  bmp: 'bmp', tiff: 'tiff', avif: 'avif', heic: 'heic', heif: 'heif',
  jxl: 'jxl', 'x-icon': 'ico', 'vnd.adobe.photoshop': 'psd',
  mp4: 'mp4', webm: 'webm', quicktime: 'mov', 'x-matroska': 'mkv',
  'x-flv': 'flv', 'x-msvideo': 'avi',
  flac: 'flac', 'x-wav': 'wav', wav: 'wav',
  pdf: 'pdf', 'epub+zip': 'epub',
};

export function mimeToExt(mime: string): string {
  const slash = mime.indexOf('/');
  if (slash < 0) return '';
  const sub = mime.slice(slash + 1).toLowerCase();
  return MIME_TO_EXT[sub] ?? sub;
}

export function drawImageCover(
  ctx: CanvasRenderingContext2D,
  img: ImageBitmap,
  dx: number, dy: number, dw: number, dh: number,
) {
  const srcAspect = img.width / img.height;
  const dstAspect = dw / dh;
  let sx: number, sy: number, sw: number, sh: number;
  if (srcAspect > dstAspect) {
    sh = img.height; sw = sh * dstAspect; sx = (img.width - sw) / 2; sy = 0;
  } else {
    sw = img.width; sh = sw / dstAspect; sx = 0; sy = (img.height - sh) / 2;
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
  ctx.drawImage(img, 0, 0, img.width, img.height, dx + (dw - sw) / 2, dy + (dh - sh) / 2, sw, sh);
}

export function getContainRect(
  aspectRatio: number,
  dx: number, dy: number, dw: number, dh: number,
): { x: number; y: number; w: number; h: number } {
  const safe = Number.isFinite(aspectRatio) && aspectRatio > 0 ? aspectRatio : 1;
  const scale = Math.min(dw / safe, dh);
  const w = safe * scale;
  const h = scale;
  return { x: dx + (dw - w) / 2, y: dy + (dh - h) / 2, w, h };
}

export function drawBadge(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number, y: number,
  align: 'left' | 'right' = 'left',
): number {
  ctx.font = BADGE_FONT;
  const metrics = ctx.measureText(text);
  const w = metrics.width + BADGE_PAD_X * 2;
  const bx = align === 'right' ? x - w : x;

  ctx.fillStyle = 'rgba(0, 0, 0, 0.65)';
  ctx.beginPath();
  ctx.roundRect(bx, y, w, BADGE_H, BADGE_RADIUS);
  ctx.fill();

  ctx.fillStyle = 'rgba(255, 255, 255, 0.80)';
  ctx.textAlign = 'left';
  ctx.textBaseline = 'middle';
  ctx.fillText(text, bx + BADGE_PAD_X, y + BADGE_H / 2);

  return w;
}

export function truncateText(
  ctx: CanvasRenderingContext2D,
  text: string,
  maxWidth: number,
): string {
  const key = `${text}\0${Math.round(maxWidth)}`;
  const cached = truncateCache.get(key);
  if (cached !== undefined) return cached;

  let result: string;
  if (ctx.measureText(text).width <= maxWidth) {
    result = text;
  } else {
    let end = text.length - 1;
    while (end > 0 && ctx.measureText(text.slice(0, end) + '…').width > maxWidth) {
      end--;
    }
    result = text.slice(0, end) + '…';
  }

  truncateCache.set(key, result);
  if (truncateCache.size > 5000) truncateCache.clear();
  return result;
}

export function formatDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  const sec = s % 60;
  if (m >= 60) {
    const h = Math.floor(m / 60);
    return `${h}:${String(m % 60).padStart(2, '0')}:${String(sec).padStart(2, '0')}`;
  }
  return `${m}:${String(sec).padStart(2, '0')}`;
}
