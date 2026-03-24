import { useMemo } from 'react';
import type { GridViewMode } from '../runtime';
import type { MasonryItem } from '../shared';

interface UseEstimatedGridTotalHeightArgs {
  exactHeight: number;
  renderImages: MasonryItem[];
  estimateSampleImages: MasonryItem[];
  totalCount: number | null;
  viewMode: GridViewMode;
  layoutWidth: number;
  targetSize: number;
  gap: number;
  textHeight: number;
  paddingX: number;
}

export function useEstimatedGridTotalHeight({
  exactHeight,
  renderImages,
  totalCount,
  viewMode,
  layoutWidth,
  targetSize,
  gap,
  textHeight,
  paddingX,
}: UseEstimatedGridTotalHeightArgs): number {
  return useMemo(() => {
    const loadedCount = renderImages.length;
    const allLoaded = !totalCount || totalCount <= loadedCount || loadedCount === 0;
    if (allLoaded) return exactHeight;

    // Grid mode: deterministic from totalCount and column geometry.
    if (viewMode === 'grid') {
      const innerWidth = Math.max(1, layoutWidth - 2 * paddingX);
      const columnCount = Math.max(1, Math.round((innerWidth + gap) / (targetSize + gap)));
      const colWidth = Math.floor((innerWidth - (columnCount - 1) * gap) / columnCount);
      const cellH = colWidth + textHeight;
      const rows = Math.ceil(totalCount / columnCount);
      return rows > 0 ? rows * cellH + (rows - 1) * gap + 4 : 0;
    }

    // Justified / waterfall: extrapolate from loaded items.
    const projected = (totalCount / loadedCount) * exactHeight;
    return Math.max(exactHeight, Math.round(projected));
  }, [
    exactHeight,
    renderImages,
    totalCount,
    viewMode,
    layoutWidth,
    targetSize,
    gap,
    textHeight,
    paddingX,
  ]);
}
