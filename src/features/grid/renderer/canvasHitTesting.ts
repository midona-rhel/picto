import type { MasonryImageItem } from '../shared';
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
  images: MasonryImageItem[];
  mouseX: number;
  mouseY: number;
  scrollTop: number;
  viewportHeight: number;
  draggedSet: Set<string>;
}): { index: number; side: 'left' | 'right' } | null {
  const { positions, images, mouseX, mouseY, scrollTop, viewportHeight, draggedSet } = args;
  const bottom = scrollTop + viewportHeight;
  for (let i = 0; i < positions.length; i++) {
    const pos = positions[i];
    if (pos.y > bottom) break;
    if (pos.y + pos.h < scrollTop) continue;
    if (mouseX >= pos.x && mouseX < pos.x + pos.w && mouseY >= pos.y && mouseY < pos.y + pos.h) {
      const img = images[i];
      if (img && draggedSet.has(img.hash)) return null;
      const midX = pos.x + pos.w / 2;
      return { index: i, side: mouseX < midX ? 'left' : 'right' };
    }
  }
  return null;
}
