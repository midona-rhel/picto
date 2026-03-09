import type { MasonryImageItem } from '../shared';
import type { GridViewMode } from '../runtime';
import type { LayoutItem } from '../layoutMath';
import {
  collectWaterfallIndices,
  getVisibleIndexRange,
  type WaterfallSeenState,
} from '../layout/canvasVisibilityPlan';

export function hitTestCanvasTile(args: {
  positions: LayoutItem[];
  mode: GridViewMode;
  mouseX: number;
  mouseY: number;
  scrollTop: number;
  viewportHeight: number;
  bucketIndex: Map<number, number[]> | null;
  waterfallSeenState: WaterfallSeenState;
  waterfallHitIndices: number[];
}): number | null {
  const {
    positions,
    mode,
    mouseX,
    mouseY,
    scrollTop,
    viewportHeight,
    bucketIndex,
    waterfallSeenState,
    waterfallHitIndices,
  } = args;
  if (mode === 'waterfall') {
    const candidates = collectWaterfallIndices(
      positions,
      scrollTop,
      scrollTop + viewportHeight,
      bucketIndex,
      waterfallSeenState,
      waterfallHitIndices,
    );
    for (let n = 0; n < candidates.length; n++) {
      const i = candidates[n];
      const pos = positions[i];
      if (mouseX >= pos.x && mouseX < pos.x + pos.w && mouseY >= pos.y && mouseY < pos.y + pos.h) {
        return i;
      }
    }
    return null;
  }

  const [startIdx, endIdx] = getVisibleIndexRange(
    positions,
    scrollTop,
    viewportHeight,
    mode,
    bucketIndex,
  );
  for (let i = startIdx; i < endIdx && i < positions.length; i++) {
    const pos = positions[i];
    if (mouseX >= pos.x && mouseX < pos.x + pos.w && mouseY >= pos.y && mouseY < pos.y + pos.h) {
      return i;
    }
  }
  return null;
}

export function computeCanvasReorderTarget(args: {
  positions: LayoutItem[];
  images: MasonryImageItem[];
  mode: GridViewMode;
  mouseX: number;
  mouseY: number;
  scrollTop: number;
  viewportHeight: number;
  bucketIndex: Map<number, number[]> | null;
  waterfallSeenState: WaterfallSeenState;
  waterfallHitIndices: number[];
  draggedSet: Set<string>;
}): { index: number; side: 'left' | 'right' } | null {
  const {
    positions,
    images,
    mode,
    mouseX,
    mouseY,
    scrollTop,
    viewportHeight,
    bucketIndex,
    waterfallSeenState,
    waterfallHitIndices,
    draggedSet,
  } = args;

  if (mode === 'waterfall') {
    const candidates = collectWaterfallIndices(
      positions,
      scrollTop,
      scrollTop + viewportHeight,
      bucketIndex,
      waterfallSeenState,
      waterfallHitIndices,
    );
    for (let n = 0; n < candidates.length; n++) {
      const i = candidates[n];
      const pos = positions[i];
      if (mouseX >= pos.x && mouseX < pos.x + pos.w && mouseY >= pos.y && mouseY < pos.y + pos.h) {
        const img = images[i];
        if (img && draggedSet.has(img.hash)) return null;
        const midX = pos.x + pos.w / 2;
        return { index: i, side: mouseX < midX ? 'left' : 'right' };
      }
    }
    return null;
  }

  const [startIdx, endIdx] = getVisibleIndexRange(
    positions,
    scrollTop,
    viewportHeight,
    mode,
    bucketIndex,
  );
  for (let i = startIdx; i < endIdx && i < positions.length; i++) {
    const pos = positions[i];
    if (mouseX >= pos.x && mouseX < pos.x + pos.w && mouseY >= pos.y && mouseY < pos.y + pos.h) {
      const img = images[i];
      if (img && draggedSet.has(img.hash)) return null;
      const midX = pos.x + pos.w / 2;
      return { index: i, side: mouseX < midX ? 'left' : 'right' };
    }
  }

  return null;
}
