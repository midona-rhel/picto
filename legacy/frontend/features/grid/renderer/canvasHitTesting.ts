import type { MasonryItem } from '../shared';
import type { LayoutItem } from '../layoutMath';

export function hitTestCanvasTile(args: {
  positions: LayoutItem[];
  mouseX: number;
  mouseY: number;
  scrollTop: number;
  viewportHeight: number;
}): number | null {
  const { positions, mouseX, mouseY, scrollTop, viewportHeight } = args;
  const bottom = scrollTop + viewportHeight;
  for (let i = 0; i < positions.length; i++) {
    const pos = positions[i];
    if (pos.y > bottom) break; // past visible range
    if (pos.y + pos.h < scrollTop) continue; // above visible range
    if (mouseX >= pos.x && mouseX < pos.x + pos.w && mouseY >= pos.y && mouseY < pos.y + pos.h) {
      return i;
    }
  }
  return null;
}

export function computeCanvasReorderTarget(args: {
  positions: LayoutItem[];
  images: MasonryItem[];
  mouseX: number;
  mouseY: number;
  scrollTop: number;
  viewportHeight: number;
  draggedSet: Set<string>;
}): { index: number; side: 'left' | 'right' } | null {
  const { positions, images, mouseX, mouseY, scrollTop, viewportHeight, draggedSet } = args;
  const bottom = scrollTop + viewportHeight;
  const visibleTargets: Array<{ index: number; pos: LayoutItem }> = [];
  let nearest: { index: number; side: 'left' | 'right'; distanceSq: number } | null = null;
  for (let i = 0; i < positions.length; i++) {
    const pos = positions[i];
    if (pos.y > bottom) break;
    if (pos.y + pos.h < scrollTop) continue;
    const img = images[i];
    if (img && draggedSet.has(img.hash)) continue;
    visibleTargets.push({ index: i, pos });

    if (mouseX >= pos.x && mouseX < pos.x + pos.w && mouseY >= pos.y && mouseY < pos.y + pos.h) {
      const midX = pos.x + pos.w / 2;
      return { index: i, side: mouseX < midX ? 'left' : 'right' };
    }
  }

  const sameBand = visibleTargets.filter(({ pos }) => mouseY >= pos.y && mouseY < pos.y + pos.h);
  if (sameBand.length > 0) {
    const nextTile = sameBand
      .filter(({ pos }) => pos.x >= mouseX)
      .sort((a, b) => a.pos.x - b.pos.x)[0];
    if (nextTile) {
      return { index: nextTile.index, side: 'left' };
    }

    const previousTile = sameBand
      .filter(({ pos }) => pos.x + pos.w <= mouseX)
      .sort((a, b) => (b.pos.x + b.pos.w) - (a.pos.x + a.pos.w))[0];
    if (previousTile) {
      return { index: previousTile.index, side: 'right' };
    }
  }

  for (const { index, pos } of visibleTargets) {

    const dy = mouseY < pos.y
      ? pos.y - mouseY
      : mouseY > pos.y + pos.h
        ? mouseY - (pos.y + pos.h)
        : 0;

    const leftDx = mouseX - pos.x;
    const leftDistanceSq = leftDx * leftDx + dy * dy;
    if (nearest == null || leftDistanceSq < nearest.distanceSq) {
      nearest = {
        index,
        side: 'left',
        distanceSq: leftDistanceSq,
      };
    }

    const rightDx = mouseX - (pos.x + pos.w);
    const rightDistanceSq = rightDx * rightDx + dy * dy;
    if (nearest == null || rightDistanceSq < nearest.distanceSq) {
      nearest = {
        index,
        side: 'right',
        distanceSq: rightDistanceSq,
      };
    }
  }
  return nearest ? { index: nearest.index, side: nearest.side } : null;
}
