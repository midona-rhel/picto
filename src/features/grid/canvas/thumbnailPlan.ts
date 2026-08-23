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
  /** Tile center Y in content space — used for viewport-distance prioritization. */
  cy: number;
}

/**
 * FNV-1a 32-bit fingerprint over the plan: first 8 chars of each file hash
 * plus its quality tier, folded with the tile count. Same tile set, order,
 * and tiers → same value. Always returns an unsigned 32-bit integer (>= 0),
 * so -1 is a safe "never computed" sentinel.
 */
export function computePlanFingerprint(tiles: PlanTile[], fullQualityThresholdPx: number): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < tiles.length; i++) {
    const t = tiles[i];
    const fileHash = t.fileHash;
    const n = Math.min(8, fileHash.length);
    for (let j = 0; j < n; j++) {
      h ^= fileHash.charCodeAt(j);
      h = Math.imul(h, 0x01000193);
    }
    h ^= t.w > fullQualityThresholdPx || t.h > fullQualityThresholdPx ? 70 : 84;
    h = Math.imul(h, 0x01000193);
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
