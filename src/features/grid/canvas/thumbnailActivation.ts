import type { LayoutItem } from '../layout/types';
import type { CanvasRenderItem } from './renderItemAdapter';
import type { PlanTile } from './thumbnailPlan';

interface ThumbnailActivationBuffers {
  activeTiles: number[];
  visibleTiles: number[];
  activeHashes: Set<string>;
  viewportHashes: Set<string>;
  planTiles: PlanTile[];
}

export function collectThumbnailActivation(
  candidates: readonly number[],
  positions: readonly LayoutItem[],
  items: readonly CanvasRenderItem[],
  activeTop: number,
  activeBottom: number,
  viewportTop: number,
  viewportBottom: number,
  buffers: ThumbnailActivationBuffers,
): void {
  buffers.activeTiles.length = 0;
  buffers.visibleTiles.length = 0;
  buffers.activeHashes.clear();
  buffers.viewportHashes.clear();
  let planCount = 0;

  for (const index of candidates) {
    const position = positions[index];
    const item = items[index];
    if (!position || !item) continue;
    if (position.y + position.h < activeTop || position.y > activeBottom) continue;

    buffers.activeTiles.push(index);
    buffers.activeHashes.add(item.thumbnailHash);
    if (position.y + position.h >= viewportTop && position.y <= viewportBottom) {
      buffers.visibleTiles.push(index);
      buffers.viewportHashes.add(item.hash);
    }

    const planTile = buffers.planTiles[planCount] ?? {
      hash: '',
      mime: '',
      w: 0,
      h: 0,
      cy: 0,
    };
    planTile.hash = item.thumbnailHash;
    planTile.mime = item.mime;
    planTile.w = position.w;
    planTile.h = position.h;
    planTile.cy = position.y + position.h / 2;
    buffers.planTiles[planCount] = planTile;
    planCount++;
  }

  buffers.planTiles.length = planCount;
}
