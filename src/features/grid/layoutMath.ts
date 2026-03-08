import type { GridViewMode } from './runtime';

export interface LayoutImageLike {
  aspectRatio: number;
}

export interface LayoutItem {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface LayoutResult {
  positions: LayoutItem[];
  totalHeight: number;
}

export interface BucketIndexEntry {
  bucket: number;
  indices: number[];
}

export const BUCKET_SIZE = 256;
const LAYOUT_PADDING_Y = 2;

export function safeAspectRatio(value: number): number {
  if (!Number.isFinite(value) || value <= 0) return 1.5;
  return Math.min(8, Math.max(0.125, value));
}

export function computeLayout(
  images: LayoutImageLike[],
  containerWidth: number,
  targetSize: number,
  gap: number,
  viewMode: GridViewMode,
  textHeight: number,
  paddingX = 0,
): LayoutResult {
  return computeLayoutFromAspectRatios(
    images.map((image) => image.aspectRatio),
    containerWidth,
    targetSize,
    gap,
    viewMode,
    textHeight,
    paddingX,
  );
}

export function computeLayoutFromAspectRatios(
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
    pos.y += LAYOUT_PADDING_Y;
  }
  result.totalHeight += LAYOUT_PADDING_Y * 2;

  return result;
}

export function buildBucketIndex(positions: LayoutItem[]): Map<number, number[]> {
  return bucketIndexEntriesToMap(buildBucketIndexEntries(positions));
}

export function buildBucketIndexEntries(positions: LayoutItem[]): BucketIndexEntry[] {
  const buckets = new Map<number, number[]>();
  for (let i = 0; i < positions.length; i++) {
    const pos = positions[i];
    const startBucket = Math.floor(pos.y / BUCKET_SIZE);
    const endBucket = Math.floor((pos.y + pos.h) / BUCKET_SIZE);
    for (let bucket = startBucket; bucket <= endBucket; bucket++) {
      let indices = buckets.get(bucket);
      if (!indices) {
        indices = [];
        buckets.set(bucket, indices);
      }
      indices.push(i);
    }
  }
  return Array.from(buckets.entries())
    .sort((a, b) => a[0] - b[0])
    .map(([bucket, indices]) => ({ bucket, indices }));
}

export function bucketIndexEntriesToMap(entries: BucketIndexEntry[]): Map<number, number[]> {
  const map = new Map<number, number[]>();
  for (const entry of entries) {
    map.set(entry.bucket, entry.indices);
  }
  return map;
}

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

    while (rowEnd < aspectRatios.length) {
      totalAspect += safeAspectRatio(aspectRatios[rowEnd]);
      rowEnd++;
      const rowWidth = totalAspect * targetRowHeight + (rowEnd - rowStart - 1) * gap;
      if (rowWidth >= containerWidth) break;
    }

    const count = rowEnd - rowStart;
    const gapSpace = (count - 1) * gap;
    const rowHeight = (containerWidth - gapSpace) / totalAspect;
    const finalHeight = Math.min(rowHeight, targetRowHeight * 1.5);
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
