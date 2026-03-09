import type { GridEmptyContext } from '../runtime';
import type { MasonryImageItem } from '../shared';

export const BADGE_H = 18;
export const BADGE_FONT = '600 10px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
export const NAME_FONT = '13px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
export const INFO_FONT = '11px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';

const BADGE_HIDDEN_TYPES = new Set(['jpg', 'jpeg', 'png', 'webp']);
const truncCache = new Map<string, string>();

export function isHiddenBadgeType(ext: string): boolean {
  return BADGE_HIDDEN_TYPES.has(ext.toLowerCase());
}

export function mimeToExt(mime: string): string {
  const slash = mime.indexOf('/');
  if (slash < 0) return '';
  const sub = mime.slice(slash + 1).toLowerCase();
  const MAP: Record<string, string> = {
    jpeg: 'jpg',
    png: 'png',
    gif: 'gif',
    webp: 'webp',
    'svg+xml': 'svg',
    mp4: 'mp4',
    webm: 'webm',
    quicktime: 'mov',
    'x-matroska': 'mkv',
    bmp: 'bmp',
    tiff: 'tiff',
    avif: 'avif',
    heic: 'heic',
  };
  return MAP[sub] ?? sub;
}

export function getEmptyStateTitle(emptyContext: GridEmptyContext, hasSearchTags: boolean): string {
  if (hasSearchTags) return 'No results found';
  if (emptyContext === 'inbox') return 'Inbox is empty';
  if (emptyContext === 'uncategorized') return 'No uncategorized images';
  if (emptyContext === 'untagged') return 'No untagged images';
  if (emptyContext === 'smart-folder') return 'No matching images';
  if (emptyContext === 'folder') return 'This folder is empty';
  return 'No images';
}

export function getEmptyStateDescription(emptyContext: GridEmptyContext, hasSearchTags: boolean): string {
  if (hasSearchTags) return 'Try different search terms or clear filters';
  if (emptyContext === 'inbox') return 'Run subscriptions to add new images to your inbox';
  if (emptyContext === 'uncategorized') return 'All your images are already assigned to folders';
  if (emptyContext === 'untagged') return 'All your images have been tagged';
  if (emptyContext === 'smart-folder') return 'Try adjusting the rules for this smart folder';
  if (emptyContext === 'folder') return 'Drag and drop files here, or import them below';
  return 'Drag and drop files here, or click the button below to import';
}

export function drawImageCover(
  ctx: CanvasRenderingContext2D,
  bitmap: ImageBitmap,
  dx: number,
  dy: number,
  dw: number,
  dh: number,
): void {
  const srcAspect = bitmap.width / bitmap.height;
  const dstAspect = dw / dh;
  let sx: number;
  let sy: number;
  let sw: number;
  let sh: number;
  if (srcAspect > dstAspect) {
    sh = bitmap.height;
    sw = sh * dstAspect;
    sx = (bitmap.width - sw) / 2;
    sy = 0;
  } else {
    sw = bitmap.width;
    sh = sw / dstAspect;
    sx = 0;
    sy = (bitmap.height - sh) / 2;
  }
  ctx.drawImage(bitmap, sx, sy, sw, sh, dx, dy, dw, dh);
}

export function drawImageContain(
  ctx: CanvasRenderingContext2D,
  bitmap: ImageBitmap,
  dx: number,
  dy: number,
  dw: number,
  dh: number,
): void {
  const scale = Math.min(dw / bitmap.width, dh / bitmap.height);
  const sw = bitmap.width * scale;
  const sh = bitmap.height * scale;
  const ox = dx + (dw - sw) / 2;
  const oy = dy + (dh - sh) / 2;
  ctx.drawImage(bitmap, 0, 0, bitmap.width, bitmap.height, ox, oy, sw, sh);
}

export function drawBadge(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
): void {
  ctx.font = BADGE_FONT;
  const metrics = ctx.measureText(text);
  const padH = 4;
  const w = metrics.width + padH * 2;
  const h = BADGE_H;
  const r = 4;

  ctx.fillStyle = 'rgba(0, 0, 0, 0.65)';
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, r);
  ctx.fill();

  ctx.fillStyle = 'rgba(255, 255, 255, 0.80)';
  ctx.textAlign = 'left';
  ctx.textBaseline = 'middle';
  ctx.fillText(text, x + padH, y + h / 2);
}

export function hasSameLayoutGeometry(
  prev: MasonryImageItem[],
  next: MasonryImageItem[],
): boolean {
  if (prev === next) return true;
  if (prev.length !== next.length) return false;
  for (let i = 0; i < prev.length; i++) {
    const a = prev[i];
    const b = next[i];
    if (a.hash !== b.hash) return false;
    if (Math.abs(a.aspectRatio - b.aspectRatio) > 0.0001) return false;
  }
  return true;
}

export function truncateText(
  ctx: CanvasRenderingContext2D,
  text: string,
  maxWidth: number,
): string {
  const key = `${text}\0${Math.round(maxWidth)}`;
  const cached = truncCache.get(key);
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

  truncCache.set(key, result);
  if (truncCache.size > 5000) {
    truncCache.clear();
  }
  return result;
}
