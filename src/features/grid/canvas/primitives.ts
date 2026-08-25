import {
  GRID_BADGE_BACKGROUND,
  GRID_BADGE_BORDER,
  GRID_BADGE_FONT,
  GRID_BADGE_TEXT,
  GRID_INFO_FONT,
  GRID_INFO_BASELINE,
  GRID_NAME_FONT,
  GRID_NAME_BASELINE,
  GRID_RATING_FONT,
  GRID_TILE_RADIUS,
} from '../gridAppearance';

export const BADGE_H = 18;
export const BADGE_FONT = GRID_BADGE_FONT;
export const BADGE_PAD_X = 2;
export const INSPECTOR_BADGE_PAD_X = 4;
export const BADGE_RADIUS = GRID_TILE_RADIUS;
export const NAME_FONT = GRID_NAME_FONT;
export const INFO_FONT = GRID_INFO_FONT;
export const NAME_BASELINE = GRID_NAME_BASELINE;
export const INFO_BASELINE = GRID_INFO_BASELINE;
export const RATING_FONT = GRID_RATING_FONT;

const truncateCache = new Map<string, string>();

const MIME_TO_EXT: Record<string, string> = {
  jpeg: 'jpg', png: 'png', gif: 'gif', webp: 'webp', 'svg+xml': 'svg',
  bmp: 'bmp', tiff: 'tiff', avif: 'avif', heic: 'heic', heif: 'heif',
  jxl: 'jpgxl', 'x-icon': 'ico', 'vnd.adobe.photoshop': 'psd',
  mp4: 'mp4', webm: 'webm', quicktime: 'mov', 'x-matroska': 'mkv',
  'x-flv': 'flv', 'x-msvideo': 'avi',
  mpeg: 'mp3', flac: 'flac', 'x-wav': 'wav', wav: 'wav', ogg: 'ogg',
  mp4a: 'm4a', aac: 'aac', opus: 'opus',
  pdf: 'pdf', 'epub+zip': 'epub', 'x-shockwave-flash': 'swf',
  'vnd.openxmlformats-officedocument.wordprocessingml.document': 'docx',
  'vnd.openxmlformats-officedocument.presentationml.presentation': 'pptx',
  'vnd.comicbook+zip': 'cbz', 'vnd.djvu': 'djvu',
  plain: 'txt', markdown: 'md', json: 'json', rtf: 'rtf',
  ttf: 'ttf', otf: 'otf', woff: 'woff', woff2: 'wof2',
};

export function mimeToExt(mime: string): string {
  const slash = mime.indexOf('/');
  if (slash < 0) return '';
  const sub = mime.slice(slash + 1).toLowerCase();
  return MIME_TO_EXT[sub] ?? sub;
}

/** Compact grid label: four characters, except explicitly named five-letter formats. */
export function formatLabelForMime(mime: string): string {
  const extension = mimeToExt(mime);
  return (extension === 'jpgxl' ? extension : extension.slice(0, 4)).toUpperCase();
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
  appearance: 'duration' | 'inspector' = 'duration',
): number {
  ctx.font = BADGE_FONT;
  const metrics = ctx.measureText(text);
  const padX = appearance === 'inspector' ? INSPECTOR_BADGE_PAD_X : BADGE_PAD_X;
  const w = metrics.width + padX * 2;
  const bx = align === 'right' ? x - w : x;

  ctx.fillStyle = GRID_BADGE_BACKGROUND;
  ctx.beginPath();
  ctx.roundRect(bx, y, w, BADGE_H, BADGE_RADIUS);
  ctx.fill();

  ctx.strokeStyle = GRID_BADGE_BORDER;
  ctx.lineWidth = 1;
  ctx.stroke();

  ctx.fillStyle = GRID_BADGE_TEXT;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'middle';
  ctx.fillText(text, bx + padX, y + BADGE_H / 2);

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
