import type { LayoutItem, LayoutResult, GridViewMode } from './types';

const LAYOUT_PADDING_TOP = 16;
const LAYOUT_PADDING_BOTTOM = 16;
export const GRID_LAYOUT_VERTICAL_PADDING = LAYOUT_PADDING_TOP + LAYOUT_PADDING_BOTTOM;

export type LayoutContinuation =
  | { mode: 'grid' }
  | { mode: 'waterfall'; columnHeights: Float64Array }
  | { mode: 'justified'; lastRowStart: number; lastRowY: number };

export interface StatefulLayoutResult extends LayoutResult {
  continuation: LayoutContinuation;
}

interface Geometry {
  fullWidth: number;
  usedWidth: number;
  columnCount: number;
  columnWidth: number;
  offsetX: number;
  snappedSize: number;
}

export function safeAspectRatio(value: number): number {
  if (!Number.isFinite(value) || value <= 0) return 1.5;
  return Math.min(8, Math.max(0.125, value));
}

export function computeLayout(
  aspectRatios: number[],
  containerWidth: number,
  targetSize: number,
  gap: number,
  viewMode: GridViewMode,
  textHeight: number,
  scrollbarWidth = 0,
): LayoutResult {
  const { continuation: _, ...result } = computeStatefulLayout(
    aspectRatios, containerWidth, targetSize, gap, viewMode, textHeight, scrollbarWidth,
  );
  return result;
}

export function computeStatefulLayout(
  aspectRatios: number[],
  containerWidth: number,
  targetSize: number,
  gap: number,
  viewMode: GridViewMode,
  textHeight: number,
  scrollbarWidth = 0,
): StatefulLayoutResult {
  if (aspectRatios.length === 0 || containerWidth <= 0) {
    return { positions: [], totalHeight: 0, continuation: { mode: viewMode } as LayoutContinuation };
  }
  const geometry = layoutGeometry(containerWidth, targetSize, gap, scrollbarWidth);
  let result: StatefulLayoutResult;
  if (viewMode === 'grid') {
    result = layoutGrid(aspectRatios.length, geometry.columnWidth, geometry.columnCount, gap, textHeight);
  } else if (viewMode === 'justified') {
    result = layoutJustified(aspectRatios, geometry.usedWidth, geometry.snappedSize, gap, textHeight);
  } else {
    result = layoutWaterfall(aspectRatios, geometry.columnWidth, geometry.columnCount, gap, textHeight);
  }
  return finalize(result, geometry.offsetX, 0);
}

export function appendLayout(
  aspectRatios: number[],
  previousCount: number,
  previous: StatefulLayoutResult,
  containerWidth: number,
  targetSize: number,
  gap: number,
  viewMode: GridViewMode,
  textHeight: number,
  scrollbarWidth = 0,
): { result: StatefulLayoutResult; stablePrefix: number } {
  if (previousCount === 0) return { result: computeStatefulLayout(
    aspectRatios, containerWidth, targetSize, gap, viewMode, textHeight, scrollbarWidth,
  ), stablePrefix: 0 };
  const geometry = layoutGeometry(containerWidth, targetSize, gap, scrollbarWidth);
  if (viewMode === 'grid' && previous.continuation.mode === 'grid') {
    const result = layoutGrid(aspectRatios.length, geometry.columnWidth, geometry.columnCount, gap, textHeight, previous.positions);
    return { result: finalize(result, geometry.offsetX, previousCount), stablePrefix: previousCount };
  }
  if (viewMode === 'waterfall' && previous.continuation.mode === 'waterfall') {
    const result = layoutWaterfall(
      aspectRatios, geometry.columnWidth, geometry.columnCount, gap, textHeight,
      previous.positions, previous.continuation.columnHeights,
    );
    return { result: finalize(result, geometry.offsetX, previousCount), stablePrefix: previousCount };
  }
  if (viewMode === 'justified' && previous.continuation.mode === 'justified') {
    const start = previous.continuation.lastRowStart;
    const result = layoutJustified(
      aspectRatios, geometry.usedWidth, geometry.snappedSize, gap, textHeight,
      previous.positions, start, previous.continuation.lastRowY,
    );
    return { result: finalize(result, geometry.offsetX, start), stablePrefix: start };
  }
  return { result: computeStatefulLayout(
    aspectRatios, containerWidth, targetSize, gap, viewMode, textHeight, scrollbarWidth,
  ), stablePrefix: 0 };
}

function layoutGeometry(containerWidth: number, targetSize: number, gap: number, scrollbarWidth: number): Geometry {
  const snappedSize = Math.max(50, Math.round(targetSize / 50) * 50);
  const fullWidth = containerWidth + scrollbarWidth;
  const minInnerWidth = fullWidth - 2 * gap;
  const columnCount = Math.max(1, Math.round((minInnerWidth + gap) / (snappedSize + gap)));
  const columnWidth = Math.floor((minInnerWidth - (columnCount - 1) * gap) / columnCount);
  const usedWidth = columnCount * columnWidth + (columnCount - 1) * gap;
  return { fullWidth, usedWidth, columnCount, columnWidth,
    offsetX: Math.floor((fullWidth - usedWidth) / 2), snappedSize };
}

function finalize(result: StatefulLayoutResult, offsetX: number, start: number): StatefulLayoutResult {
  for (let index = start; index < result.positions.length; index++) {
    result.positions[index].x += offsetX;
    result.positions[index].y += LAYOUT_PADDING_TOP;
  }
  result.totalHeight += GRID_LAYOUT_VERTICAL_PADDING;
  return result;
}

function layoutWaterfall(
  aspectRatios: number[],
  colWidth: number,
  columnCount: number,
  gap: number,
  textHeight: number,
  previous: LayoutItem[] = [],
  previousHeights: Float64Array = new Float64Array(columnCount),
): StatefulLayoutResult {
  const start = previous.length;
  const colHeights = previousHeights.slice();
  const positions = previous.slice();
  positions.length = aspectRatios.length;

  for (let i = start; i < aspectRatios.length; i++) {
    let shortest = 0;
    for (let c = 1; c < columnCount; c++) {
      if (colHeights[c] < colHeights[shortest]) shortest = c;
    }

    const x = shortest * (colWidth + gap);
    const y = colHeights[shortest];
    const h = colWidth / safeAspectRatio(aspectRatios[i]) + textHeight;

    positions[i] = { x, y, w: colWidth, h };
    colHeights[shortest] = y + h + gap;
  }

  let maxHeight = 0;
  for (let c = 0; c < columnCount; c++) {
    if (colHeights[c] > maxHeight) maxHeight = colHeights[c];
  }

  return { positions, totalHeight: Math.max(0, maxHeight - gap),
    continuation: { mode: 'waterfall', columnHeights: colHeights } };
}

function layoutGrid(
  imageCount: number,
  colWidth: number,
  columnCount: number,
  gap: number,
  textHeight: number,
  previous: LayoutItem[] = [],
): StatefulLayoutResult {
  const start = previous.length;
  const positions = previous.slice();
  positions.length = imageCount;
  const tileSize = colWidth;
  const cellH = tileSize + textHeight;

  for (let i = start; i < imageCount; i++) {
    const col = i % columnCount;
    const row = Math.floor(i / columnCount);
    positions[i] = {
      x: col * (tileSize + gap),
      y: row * (cellH + gap),
      w: tileSize,
      h: cellH,
    };
  }

  const rows = Math.ceil(imageCount / columnCount);
  const totalHeight = rows > 0 ? rows * cellH + (rows - 1) * gap : 0;
  return { positions, totalHeight, continuation: { mode: 'grid' } };
}

function layoutJustified(
  aspectRatios: number[],
  containerWidth: number,
  targetRowHeight: number,
  gap: number,
  textHeight: number,
  previous: LayoutItem[] = [],
  rowStart = 0,
  y = 0,
): StatefulLayoutResult {
  const positions = previous.slice(0, rowStart);
  positions.length = aspectRatios.length;
  let lastRowStart = rowStart;
  let lastRowY = y;

  while (rowStart < aspectRatios.length) {
    lastRowStart = rowStart;
    lastRowY = y;
    let rowEnd = rowStart;
    let totalAspect = 0;

    while (rowEnd < aspectRatios.length) {
      totalAspect += safeAspectRatio(aspectRatios[rowEnd]);
      rowEnd++;
      const rowWidth = totalAspect * targetRowHeight + (rowEnd - rowStart - 1) * gap;
      if (rowWidth >= containerWidth) break;
    }

    // An incomplete final row keeps the target height. If its final tile is
    // also the tile that crosses the available width, that rule would make it
    // paint beyond the canvas. Finish the preceding row and let the tile begin
    // a new final row instead.
    const targetWidth = totalAspect * targetRowHeight + (rowEnd - rowStart - 1) * gap;
    if (rowEnd === aspectRatios.length && rowEnd - rowStart > 1 && targetWidth > containerWidth) {
      rowEnd--;
      totalAspect -= safeAspectRatio(aspectRatios[rowEnd]);
    }

    const isLastRow = rowEnd === aspectRatios.length;
    const count = rowEnd - rowStart;
    const gapSpace = (count - 1) * gap;

    const finalHeight = isLastRow
      ? targetRowHeight
      : Math.min((containerWidth - gapSpace) / totalAspect, targetRowHeight * 1.5);

    const cellH = finalHeight + textHeight;

    let x = 0;
    for (let i = rowStart; i < rowEnd; i++) {
      const ar = safeAspectRatio(aspectRatios[i]);
      let w = finalHeight * ar;
      let h = cellH;
      if (w > containerWidth) {
        w = containerWidth;
        h = w / ar + textHeight;
      }
      positions[i] = { x, y, w, h };
      x += w + gap;
    }

    y += cellH + gap;
    rowStart = rowEnd;
  }

  return { positions, totalHeight: Math.max(0, y - gap),
    continuation: { mode: 'justified', lastRowStart, lastRowY } };
}
