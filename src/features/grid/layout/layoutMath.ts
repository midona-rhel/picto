/**
 * Grid layout math — pure computation, no DOM or React dependencies.
 *
 * Three layout modes:
 *   - waterfall: masonry (shortest-column placement)
 *   - grid: uniform square tiles
 *   - justified: row-filling with aspect-ratio-aware sizing
 *
 * Ported from legacy/frontend/features/grid/layoutMath.ts with justified
 * last-row fix: incomplete last rows use target height instead of stretching.
 */

import type { LayoutItem, LayoutResult, GridViewMode } from './types';

const LAYOUT_PADDING_TOP = 20;
const LAYOUT_PADDING_BOTTOM = 0;

export function safeAspectRatio(value: number): number {
  if (!Number.isFinite(value) || value <= 0) return 1.5;
  return Math.min(8, Math.max(0.125, value));
}

/**
 * Compute layout positions for all items.
 *
 * @param aspectRatios - aspect ratio per item (width/height)
 * @param containerWidth - available width in pixels
 * @param targetSize - target tile width/height in pixels (100–900)
 * @param gap - pixel gap between tiles
 * @param viewMode - waterfall, grid, or justified
 * @param textHeight - extra height below each tile for name/resolution text
 * @param paddingX - horizontal inset from container edges
 */
export function computeLayout(
  aspectRatios: number[],
  containerWidth: number,
  targetSize: number,
  gap: number,
  viewMode: GridViewMode,
  textHeight: number,
  paddingX = 0,
): LayoutResult {
  if (aspectRatios.length === 0 || containerWidth <= 0) {
    return { positions: [], totalHeight: 0 };
  }

  const innerWidth = containerWidth - 2 * paddingX;
  const columnCount = Math.max(1, Math.round((innerWidth + gap) / (targetSize + gap)));
  const colWidth = Math.floor((innerWidth - (columnCount - 1) * gap) / columnCount);

  let result: LayoutResult;
  if (viewMode === 'grid') {
    result = layoutGrid(aspectRatios.length, colWidth, columnCount, gap, textHeight);
  } else if (viewMode === 'justified') {
    result = layoutJustified(aspectRatios, innerWidth, targetSize, gap, textHeight);
  } else {
    result = layoutWaterfall(aspectRatios, colWidth, columnCount, gap, textHeight);
  }

  for (const pos of result.positions) {
    if (paddingX > 0) pos.x += paddingX;
    pos.y += LAYOUT_PADDING_TOP;
  }
  result.totalHeight += LAYOUT_PADDING_TOP + LAYOUT_PADDING_BOTTOM;

  return result;
}

// ── Waterfall (masonry) ──────────────────────────────────────────

function layoutWaterfall(
  aspectRatios: number[],
  colWidth: number,
  columnCount: number,
  gap: number,
  textHeight: number,
): LayoutResult {
  const colHeights = new Float64Array(columnCount);
  const positions: LayoutItem[] = new Array(aspectRatios.length);

  for (let i = 0; i < aspectRatios.length; i++) {
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

  return { positions, totalHeight: Math.max(0, maxHeight - gap) };
}

// ── Grid (uniform squares) ───────────────────────────────────────

function layoutGrid(
  imageCount: number,
  colWidth: number,
  columnCount: number,
  gap: number,
  textHeight: number,
): LayoutResult {
  const positions: LayoutItem[] = new Array(imageCount);
  const tileSize = colWidth;
  const cellH = tileSize + textHeight;

  for (let i = 0; i < imageCount; i++) {
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
  return { positions, totalHeight };
}

// ── Justified (row-filling) ──────────────────────────────────────

function layoutJustified(
  aspectRatios: number[],
  containerWidth: number,
  targetRowHeight: number,
  gap: number,
  textHeight: number,
): LayoutResult {
  const positions: LayoutItem[] = new Array(aspectRatios.length);
  let y = 0;
  let rowStart = 0;

  while (rowStart < aspectRatios.length) {
    let rowEnd = rowStart;
    let totalAspect = 0;

    // Fill the row until it reaches or exceeds the container width
    while (rowEnd < aspectRatios.length) {
      totalAspect += safeAspectRatio(aspectRatios[rowEnd]);
      rowEnd++;
      const rowWidth = totalAspect * targetRowHeight + (rowEnd - rowStart - 1) * gap;
      if (rowWidth >= containerWidth) break;
    }

    const isLastRow = rowEnd === aspectRatios.length;
    const count = rowEnd - rowStart;
    const gapSpace = (count - 1) * gap;

    let finalHeight: number;
    if (isLastRow) {
      // Last incomplete row: use target height, don't stretch.
      // Items are left-aligned at their natural width.
      finalHeight = targetRowHeight;
    } else {
      // Full row: compute height that makes items fill the container width exactly.
      const rowHeight = (containerWidth - gapSpace) / totalAspect;
      finalHeight = Math.min(rowHeight, targetRowHeight * 1.5);
    }

    const cellH = finalHeight + textHeight;

    let x = 0;
    for (let i = rowStart; i < rowEnd; i++) {
      const w = finalHeight * safeAspectRatio(aspectRatios[i]);
      positions[i] = { x, y, w, h: cellH };
      x += w + gap;
    }

    y += cellH + gap;
    rowStart = rowEnd;
  }

  return { positions, totalHeight: Math.max(0, y - gap) };
}

/** Binary search: find first index where positions[i].y + h >= target. */
export function lowerBound(positions: LayoutItem[], targetY: number): number {
  let lo = 0;
  let hi = positions.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (positions[mid].y + positions[mid].h < targetY) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}
