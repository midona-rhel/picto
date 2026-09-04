import type { GridLayoutModel } from './gridLayoutModel';
import { buildTileSpatialIndex } from '../layout/spatialIndex';

export const GRID_SCROLL_SETTLE_MS = 40;
// Keep the physical scrollbar below browser layout-coordinate limits. Loaded
// tiles keep their real pixel sizes; the unloaded span is only an estimate.
export const MAX_GRID_SCROLL_HEIGHT = 8_000_000;

export interface GridWindowModel extends GridLayoutModel {
  windowTop: number;
  windowBottom: number;
}

export function placeGridWindow(
  local: GridLayoutModel,
  start: number,
  total: number,
  pixelsPerItem: number,
  anchor?: { index: number; top: number } | null,
): GridWindowModel {
  const count = local.items.length;
  const height = total <= count ? local.totalHeight
    : Math.max(local.totalHeight, Math.min(MAX_GRID_SCROLL_HEIGHT, total * pixelsPerItem));
  const remaining = Math.max(0, total - count);
  let top = remaining > 0 ? start / remaining * (height - local.totalHeight) : 0;
  const anchorPosition = anchor && local.positions[anchor.index - start];
  if (anchorPosition) top = anchor.top - anchorPosition.y;
  // The first/last windows must expose the actual start/end, including when
  // their aspect-ratio distribution differs from the estimate.
  if (start === 0) top = 0;
  else if (start + count >= total) top = height - local.totalHeight;
  top = Math.max(0, Math.min(height - local.totalHeight, top));
  const positions = local.positions.map(position => ({ ...position, y: position.y + top }));
  return { ...local, positions, spatialIndex: buildTileSpatialIndex(positions),
    totalHeight: height, windowTop: top, windowBottom: top + local.totalHeight };
}

/** Choose an exact ordinal to request; geometry alone is approximate. */
export function gridWindowDestination(
  model: GridWindowModel, start: number, total: number, scrollTop: number, viewportHeight: number,
): number | null {
  if (total <= model.items.length || viewportHeight <= 0) return null;
  const center = scrollTop + viewportHeight / 2;
  if (center >= model.windowTop && center < model.windowBottom) {
    const nearStart = start > 0 && scrollTop < model.windowTop + viewportHeight * 3;
    const nearEnd = start + model.items.length < total
      && scrollTop + viewportHeight > model.windowBottom - viewportHeight * 3;
    if (!nearStart && !nearEnd) return null;
    let nearest = 0;
    for (let i = 1; i < model.positions.length; i++) {
      if (Math.abs(model.positions[i].y - center) < Math.abs(model.positions[nearest].y - center)) nearest = i;
    }
    return start + nearest;
  }
  const progress = Math.max(0, Math.min(1, scrollTop / Math.max(1, model.totalHeight - viewportHeight)));
  return Math.min(total - 1, Math.floor(progress * total));
}

/** True when the viewport center has no resident tiles to paint. */
export function gridViewportMissesWindow(
  model: GridWindowModel,
  scrollTop: number,
  viewportHeight: number,
): boolean {
  if (viewportHeight <= 0 || model.items.length === 0) return false;
  const center = scrollTop + viewportHeight / 2;
  return center < model.windowTop || center >= model.windowBottom;
}

/** No fetch until the latest scroll has been quiet for the full 40 ms. */
export class SettledGridRequest {
  private timer: ReturnType<typeof setTimeout> | null = null;
  schedule(callback: () => void): void {
    this.cancel();
    this.timer = setTimeout(() => { this.timer = null; callback(); }, GRID_SCROLL_SETTLE_MS);
  }
  cancel(): void {
    if (this.timer != null) clearTimeout(this.timer);
    this.timer = null;
  }
}
