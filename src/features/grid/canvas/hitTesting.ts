/**
 * Canvas hit testing — find which tile a pointer event targets.
 */

import type { LayoutItem } from '../layout/types';

/** Returns the item index at the given canvas coordinates, or null. */
export function hitTestTile(
  positions: LayoutItem[],
  x: number,
  y: number,
  textHeight: number,
  visibleStart: number,
  visibleEnd: number,
): number | null {
  for (let i = visibleStart; i < visibleEnd && i < positions.length; i++) {
    const pos = positions[i];
    const imgH = pos.h - textHeight;
    if (x >= pos.x && x <= pos.x + pos.w && y >= pos.y && y <= pos.y + imgH) {
      return i;
    }
  }
  return null;
}

export interface ReorderTarget {
  index: number;
  side: 'left' | 'right';
}

/** Compute where a dragged item would be inserted in the grid.
 *  `skipIndices` contains indices of the dragged tiles — they're excluded from targeting. */
export function computeReorderTarget(
  positions: LayoutItem[],
  mouseX: number,
  mouseY: number,
  textHeight: number,
  skipIndices?: Set<number>,
): ReorderTarget | null {
  if (positions.length === 0) return null;
  const skip = skipIndices ?? new Set<number>();

  // 1. Direct hit — cursor is over a non-dragged tile → split at midpoint
  for (let i = 0; i < positions.length; i++) {
    if (skip.has(i)) continue;
    const pos = positions[i];
    const imgH = pos.h - textHeight;
    if (mouseX >= pos.x && mouseX <= pos.x + pos.w && mouseY >= pos.y && mouseY <= pos.y + imgH) {
      const mid = pos.x + pos.w / 2;
      return { index: i, side: mouseX < mid ? 'left' : 'right' };
    }
  }

  // 2. Same band — collect non-skipped tiles at cursor's Y, find gap
  const bandTiles: { idx: number; pos: LayoutItem }[] = [];
  for (let i = 0; i < positions.length; i++) {
    if (skip.has(i)) continue;
    const pos = positions[i];
    const imgH = pos.h - textHeight;
    if (mouseY >= pos.y && mouseY <= pos.y + imgH) {
      bandTiles.push({ idx: i, pos });
    }
  }
  bandTiles.sort((a, b) => a.pos.x - b.pos.x);

  if (bandTiles.length > 0) {
    // Left of first tile
    if (mouseX < bandTiles[0].pos.x) {
      return { index: bandTiles[0].idx, side: 'left' };
    }
    // Right of last tile
    const last = bandTiles[bandTiles.length - 1];
    if (mouseX > last.pos.x + last.pos.w) {
      return { index: last.idx, side: 'right' };
    }
    // In a gap between two tiles
    for (let j = 0; j < bandTiles.length - 1; j++) {
      const lt = bandTiles[j];
      const rt = bandTiles[j + 1];
      if (mouseX >= lt.pos.x + lt.pos.w && mouseX <= rt.pos.x) {
        const gapMid = (lt.pos.x + lt.pos.w + rt.pos.x) / 2;
        return mouseX < gapMid
          ? { index: lt.idx, side: 'right' }
          : { index: rt.idx, side: 'left' };
      }
    }
  }

  // 3. Fallback — find nearest non-dragged tile by distance
  let bestDist = Infinity;
  let bestTarget: ReorderTarget | null = null;
  for (let i = 0; i < positions.length; i++) {
    if (skip.has(i)) continue;
    const pos = positions[i];
    const cx = pos.x + pos.w / 2;
    const cy = pos.y + (pos.h - textHeight) / 2;
    const dist = (mouseX - cx) ** 2 + (mouseY - cy) ** 2;
    if (dist < bestDist) {
      bestDist = dist;
      const mid = pos.x + pos.w / 2;
      bestTarget = { index: i, side: mouseX < mid ? 'left' : 'right' };
    }
  }
  return bestTarget;
}
