export const THUMBNAIL_PIPELINE_MAX_ENTRIES = 2000;
export const THUMBNAIL_PIPELINE_MAX_ACTIVE_IDLE = 6;
export const THUMBNAIL_PIPELINE_MAX_ACTIVE_SCROLL = 3;
export const THUMBNAIL_PIPELINE_DECODE_MAX_SIDE_SCROLL = 256;
export const THUMBNAIL_PIPELINE_DECODE_MAX_SIDE_IDLE = 384;

export function clampThumbnailDecodeSide(mime: string, scrolling: boolean): number {
  const base = scrolling
    ? THUMBNAIL_PIPELINE_DECODE_MAX_SIDE_SCROLL
    : THUMBNAIL_PIPELINE_DECODE_MAX_SIDE_IDLE;
  if (mime === 'image/avif') return Math.min(base, 256);
  if (mime === 'image/webp') return Math.min(base, 320);
  return base;
}
