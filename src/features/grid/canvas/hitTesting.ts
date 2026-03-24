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
