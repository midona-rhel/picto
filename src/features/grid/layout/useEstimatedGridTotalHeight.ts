import { useMemo, useRef } from 'react';
import type { GridViewMode } from '../runtime';
import type { MasonryImageItem } from '../shared';

interface EstimateInputSnapshot {
  totalCount: number | null;
  imagesLen: number;
  viewMode: GridViewMode;
  layoutWidth: number;
}

interface UseEstimatedGridTotalHeightArgs {
  exactHeight: number;
  renderImages: MasonryImageItem[];
  estimateSampleImages: MasonryImageItem[];
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
  estimateSampleImages,
  totalCount,
  viewMode,
  layoutWidth,
  targetSize,
  gap,
  textHeight,
  paddingX,
}: UseEstimatedGridTotalHeightArgs): number {
  const estimateRef = useRef(0);
  const prevEstimateInputRef = useRef<EstimateInputSnapshot>({
    totalCount: null,
    imagesLen: 0,
    viewMode,
    layoutWidth: 0,
  });

  return useMemo(() => {
    const loadedAll = !totalCount || totalCount <= renderImages.length || renderImages.length === 0;
    if (loadedAll) {
      estimateRef.current = exactHeight;
      prevEstimateInputRef.current = { totalCount, imagesLen: renderImages.length, viewMode, layoutWidth };
      return exactHeight;
    }

    const innerWidth = Math.max(1, layoutWidth - 2 * paddingX);
    const clampAspect = (value: number): number => {
      if (!Number.isFinite(value) || value <= 0) return 1.5;
      return Math.min(8, Math.max(0.125, value));
    };
    const loadedSample = renderImages.slice(Math.max(0, renderImages.length - 220));
    const loadedHashes = new Set(loadedSample.map((item) => item.hash));
    const lookaheadSample = estimateSampleImages
      .slice(0, 120)
      .filter((item) => !loadedHashes.has(item.hash));
    const estimatePool = lookaheadSample.length > 0 ? [...loadedSample, ...lookaheadSample] : loadedSample;

    let projected = exactHeight;
    if (viewMode === 'grid') {
      const columnCount = Math.max(1, Math.round((innerWidth + gap) / (targetSize + gap)));
      const colWidth = Math.floor((innerWidth - (columnCount - 1) * gap) / columnCount);
      const cellH = colWidth + textHeight;
      const rows = Math.ceil(totalCount / columnCount);
      projected = rows > 0 ? rows * cellH + (rows - 1) * gap + 4 : 0;
    } else {
      let avgHeightPerItem = exactHeight / Math.max(1, renderImages.length);
      if (estimatePool.length > 0) {
        if (viewMode === 'waterfall') {
          const columnCount = Math.max(1, Math.round((innerWidth + gap) / (targetSize + gap)));
          const colWidth = Math.floor((innerWidth - (columnCount - 1) * gap) / columnCount);
          let sumH = 0;
          for (const image of estimatePool) {
            sumH += (colWidth / clampAspect(image.aspectRatio)) + textHeight;
          }
          const avgItemH = sumH / estimatePool.length;
          avgHeightPerItem = ((avgItemH + gap) / columnCount) * 1.04;
        } else if (viewMode === 'justified') {
          let y = 0;
          let rowStart = 0;
          while (rowStart < estimatePool.length) {
            let rowEnd = rowStart;
            let totalAspect = 0;
            while (rowEnd < estimatePool.length) {
              totalAspect += clampAspect(estimatePool[rowEnd].aspectRatio);
              rowEnd++;
              const rowWidth = totalAspect * targetSize + (rowEnd - rowStart - 1) * gap;
              if (rowWidth >= innerWidth) break;
            }
            const count = rowEnd - rowStart;
            const gapSpace = (count - 1) * gap;
            const rowHeight = (innerWidth - gapSpace) / Math.max(0.001, totalAspect);
            const finalHeight = Math.min(rowHeight, targetSize * 1.5);
            y += finalHeight + textHeight + gap;
            rowStart = rowEnd;
          }
          const totalH = Math.max(0, y - gap);
          avgHeightPerItem = Math.max(1, totalH / estimatePool.length);
        }
      }
      projected = Math.max(exactHeight, Math.round(avgHeightPerItem * totalCount));
    }
    projected = Math.max(exactHeight, projected);

    const prev = estimateRef.current || projected;
    const prevInput = prevEstimateInputRef.current;
    const modeChanged = prevInput.viewMode !== viewMode;
    const widthChanged = prevInput.layoutWidth > 0 && Math.abs(prevInput.layoutWidth - layoutWidth) > 12;
    const resetEstimate =
      modeChanged ||
      widthChanged ||
      (prevInput.totalCount !== null &&
        totalCount < prevInput.totalCount &&
        renderImages.length <= prevInput.imagesLen);

    if (resetEstimate) {
      estimateRef.current = projected;
      prevEstimateInputRef.current = { totalCount, imagesLen: renderImages.length, viewMode, layoutWidth };
      return projected;
    }

    let next = prev;
    if (viewMode === 'grid') {
      next = projected;
    } else if (viewMode === 'justified') {
      if (projected > prev) {
        const delta = projected - prev;
        next = delta < 48 ? projected : prev + Math.max(24, Math.round(delta * 0.35));
      } else if (projected < prev) {
        const delta = prev - projected;
        next = delta < 48 ? projected : prev - Math.max(24, Math.round(delta * 0.35));
      }
    } else if (viewMode === 'waterfall') {
      if (projected > prev) {
        const delta = projected - prev;
        next = delta < 48 ? projected : prev + Math.max(24, Math.round(delta * 0.28));
      } else if (projected < prev) {
        const delta = prev - projected;
        next = delta < 48 ? projected : prev - Math.max(16, Math.round(delta * 0.18));
      }
    }
    next = Math.max(next, exactHeight);

    estimateRef.current = next;
    prevEstimateInputRef.current = { totalCount, imagesLen: renderImages.length, viewMode, layoutWidth };
    return next;
  }, [
    exactHeight,
    renderImages,
    estimateSampleImages,
    totalCount,
    viewMode,
    layoutWidth,
    targetSize,
    gap,
    textHeight,
    paddingX,
  ]);
}
