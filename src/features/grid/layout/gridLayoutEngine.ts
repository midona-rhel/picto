import { computeTextHeight, TEXT_NAME_ROW_H, TEXT_RESOLUTION_ROW_H } from '../gridLayout';
import { useWaterfallLayoutWorker } from '../hooks/useWaterfallLayoutWorker';
import type { MasonryImageItem } from '../shared';
import type { GridViewMode } from '../runtime';

interface UseGridLayoutEngineArgs {
  images: MasonryImageItem[];
  layoutWidth: number;
  targetSize: number;
  gap: number;
  viewMode: GridViewMode;
  showTileName: boolean;
  showResolution: boolean;
  paddingX: number;
}

export function useGridLayoutEngine({
  images,
  layoutWidth,
  targetSize,
  gap,
  viewMode,
  showTileName,
  showResolution,
  paddingX,
}: UseGridLayoutEngineArgs) {
  const textHeight = computeTextHeight(showTileName, showResolution);
  const { renderImages, layout, bucketIndex } = useWaterfallLayoutWorker({
    images,
    layoutWidth,
    targetSize,
    gap,
    viewMode,
    textHeight,
    paddingX,
  });

  return {
    textHeight,
    renderImages,
    layout,
    bucketIndex,
  };
}

export {
  computeTextHeight,
  TEXT_NAME_ROW_H,
  TEXT_RESOLUTION_ROW_H,
};
