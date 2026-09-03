/**
 * Pure helpers for the thumbnail plan — fingerprinting and prioritization.
 *
 * Kept free of worker/DOM dependencies so they can be unit-tested.
 */

export interface PlanTile {
  fileHash: string;
  mime: string;
  w: number;
  h: number;
  sourceWidth?: number | null;
  sourceHeight?: number | null;
  fit?: 'cover' | 'contain';
  inViewport?: boolean;
  fullQualityEligible?: boolean;
  /** Tile center Y in content space — used for viewport-distance prioritization. */
  cy: number;
}

export interface FullQualityDecodeSize {
  width: number;
  height: number;
}

/** Decode only the pixels the grid can display while preserving the source aspect ratio. */
export function fullQualityDecodeSize(
  tile: PlanTile,
  devicePixelRatio = 1,
): FullQualityDecodeSize {
  const sourceWidth = tile.sourceWidth ?? 0;
  const sourceHeight = tile.sourceHeight ?? 0;
  const dpr = Math.max(1, devicePixelRatio);
  if (sourceWidth > 0 && sourceHeight > 0) {
    const scale = tile.fit === 'contain'
      ? Math.min(tile.w / sourceWidth, tile.h / sourceHeight)
      : Math.max(tile.w / sourceWidth, tile.h / sourceHeight);
    return {
      width: Math.max(1, Math.min(sourceWidth, Math.ceil(sourceWidth * scale * dpr))),
      height: Math.max(1, Math.min(sourceHeight, Math.ceil(sourceHeight * scale * dpr))),
    };
  }
  return {
    width: Math.max(1, Math.ceil(tile.w * dpr)),
    height: Math.max(1, Math.ceil(tile.h * dpr)),
  };
}

/** Originals are only useful when Chromium can decode them into an ImageBitmap. */
export function shouldLoadFullQualityOriginal(
  tile: PlanTile,
  thumbnailLongEdgePx: number,
  devicePixelRatio = 1,
): boolean {
  if (tile.fullQualityEligible === false
    || !tile.mime.startsWith('image/')
    || tile.mime === 'image/gif'
    || tile.mime === 'image/jxl') {
    return false;
  }
  const required = fullQualityDecodeSize(tile, devicePixelRatio);
  const sourceWidth = tile.sourceWidth ?? 0;
  const sourceHeight = tile.sourceHeight ?? 0;
  if (sourceWidth > 0 && sourceHeight > 0) {
    const thumbnailScale = Math.min(1, thumbnailLongEdgePx / Math.max(sourceWidth, sourceHeight));
    return required.width > Math.max(1, Math.floor(sourceWidth * thumbnailScale))
      || required.height > Math.max(1, Math.floor(sourceHeight * thumbnailScale));
  }
  return required.width > thumbnailLongEdgePx || required.height > thumbnailLongEdgePx;
}

/**
 * FNV-1a 32-bit fingerprint over the plan: first 8 chars of each file hash
 * plus its quality tier, folded with the tile count. Same tile set, order,
 * and tiers → same value. Always returns an unsigned 32-bit integer (>= 0),
 * so -1 is a safe "never computed" sentinel.
 */
export function computePlanFingerprint(
  tiles: PlanTile[],
  thumbnailLongEdgePx: number,
  devicePixelRatio = 1,
): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < tiles.length; i++) {
    const t = tiles[i];
    const fileHash = t.fileHash;
    const n = Math.min(8, fileHash.length);
    for (let j = 0; j < n; j++) {
      h ^= fileHash.charCodeAt(j);
      h = Math.imul(h, 0x01000193);
    }
    const needsFull = shouldLoadFullQualityOriginal(t, thumbnailLongEdgePx, devicePixelRatio);
    h ^= needsFull ? 70 : 84;
    h = Math.imul(h, 0x01000193);
    if (needsFull) {
      const decodeSize = fullQualityDecodeSize(t, devicePixelRatio);
      h ^= decodeSize.width;
      h = Math.imul(h, 0x01000193);
      h ^= decodeSize.height;
      h = Math.imul(h, 0x01000193);
    }
  }
  h ^= tiles.length;
  h = Math.imul(h, 0x01000193);
  return h >>> 0;
}

/**
 * Sort tiles in place by distance from the viewport center (nearest first).
 * Plan-entry order is the worker's fetch priority, so this makes tiles under
 * the user's eyes load before tiles at the activation-zone edges. Y-only:
 * the viewport spans the grid's full width, so X distance adds nothing.
 */
export function sortPlanTilesByViewportDistance(tiles: PlanTile[], viewportCenterY: number): void {
  tiles.sort((a, b) => Math.abs(a.cy - viewportCenterY) - Math.abs(b.cy - viewportCenterY));
}
