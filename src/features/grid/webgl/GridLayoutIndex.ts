import { safeAspectRatio } from '../layout/layoutMath';
import type { LayoutItem, GridViewMode } from '../layout/types';

const LAYOUT_PADDING_TOP = 20;
const LAYOUT_PADDING_BOTTOM = 0;
const BLOCK_SIZE = 512;

export interface GridLayoutRange {
  renderedIndices: number[];
  visibleIndices: number[];
  totalHeight: number;
}

interface LayoutBlock {
  startIndex: number;
  positions: LayoutItem[];
  minY: number;
  maxBottom: number;
}

interface InvalidateArgs {
  itemCount: number;
  getAspectRatio: (index: number) => number;
  containerWidth: number;
  targetSize: number;
  gap: number;
  viewMode: GridViewMode;
  textHeight: number;
  paddingX?: number;
  scrollbarWidth?: number;
}

function lowerBoundBlocks(
  blocks: LayoutBlock[],
  target: number,
  selector: (block: LayoutBlock) => number,
): number {
  let lo = 0;
  let hi = blocks.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (selector(blocks[mid]) < target) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

export class GridLayoutIndex {
  private blocks: LayoutBlock[] = [];
  private itemCount = 0;
  private totalHeight = 0;

  invalidate(args: InvalidateArgs): void {
    const {
      itemCount,
      getAspectRatio,
      containerWidth,
      targetSize,
      gap,
      viewMode,
      textHeight,
      paddingX = 0,
      scrollbarWidth = 0,
    } = args;

    this.itemCount = itemCount;
    this.blocks = [];
    this.totalHeight = 0;

    if (itemCount <= 0 || containerWidth <= 0) {
      return;
    }

    const fullWidth = containerWidth + scrollbarWidth;
    const minInnerWidth = fullWidth - 2 * gap;
    const columnCount = Math.max(1, Math.round((minInnerWidth + gap) / (targetSize + gap)));
    const colWidth = Math.floor((minInnerWidth - (columnCount - 1) * gap) / columnCount);
    const usedWidth = columnCount * colWidth + (columnCount - 1) * gap;
    const offsetX = Math.max(0, Math.floor((fullWidth - usedWidth) / 2) + paddingX);

    if (viewMode === 'grid') {
      this.buildGrid(itemCount, colWidth, columnCount, gap, textHeight, offsetX);
      return;
    }
    if (viewMode === 'justified') {
      this.buildJustified(itemCount, getAspectRatio, usedWidth, targetSize, gap, textHeight, offsetX);
      return;
    }
    this.buildWaterfall(itemCount, getAspectRatio, colWidth, columnCount, gap, textHeight, offsetX);
  }

  getItem(index: number): LayoutItem | null {
    if (index < 0 || index >= this.itemCount) return null;
    const blockIndex = Math.floor(index / BLOCK_SIZE);
    const block = this.blocks[blockIndex];
    if (!block) return null;
    return block.positions[index - block.startIndex] ?? null;
  }

  getRange(scrollTop: number, viewportHeight: number, overscanPx: number): GridLayoutRange {
    if (this.blocks.length === 0 || viewportHeight <= 0) {
      return { renderedIndices: [], visibleIndices: [], totalHeight: this.totalHeight };
    }

    const renderTop = scrollTop - overscanPx;
    const renderBottom = scrollTop + viewportHeight + overscanPx;
    const visibleTop = scrollTop;
    const visibleBottom = scrollTop + viewportHeight;
    const renderedIndices: number[] = [];
    const visibleIndices: number[] = [];

    const startBlock = Math.max(0, lowerBoundBlocks(this.blocks, renderTop, (block) => block.maxBottom) - 1);
    for (let blockIndex = startBlock; blockIndex < this.blocks.length; blockIndex += 1) {
      const block = this.blocks[blockIndex];
      if (block.maxBottom < renderTop) continue;
      if (block.minY > renderBottom) break;

      for (let i = 0; i < block.positions.length; i += 1) {
        const index = block.startIndex + i;
        const position = block.positions[i];
        const bottom = position.y + position.h;
        if (bottom < renderTop) continue;
        if (position.y > renderBottom) break;
        renderedIndices.push(index);
        if (bottom >= visibleTop && position.y <= visibleBottom) {
          visibleIndices.push(index);
        }
      }
    }

    return { renderedIndices, visibleIndices, totalHeight: this.totalHeight };
  }

  getTotalHeight(): number {
    return this.totalHeight;
  }

  private pushBlock(startIndex: number, positions: LayoutItem[]): void {
    if (positions.length === 0) return;
    let minY = positions[0].y;
    let maxBottom = positions[0].y + positions[0].h;
    for (let i = 1; i < positions.length; i += 1) {
      const position = positions[i];
      if (position.y < minY) minY = position.y;
      const bottom = position.y + position.h;
      if (bottom > maxBottom) maxBottom = bottom;
    }
    this.blocks.push({ startIndex, positions, minY, maxBottom });
  }

  private buildGrid(
    itemCount: number,
    colWidth: number,
    columnCount: number,
    gap: number,
    textHeight: number,
    offsetX: number,
  ): void {
    let blockStart = 0;
    let blockPositions: LayoutItem[] = [];
    const tileSize = colWidth;
    const cellH = tileSize + textHeight;

    for (let i = 0; i < itemCount; i += 1) {
      if (blockPositions.length === 0) blockStart = i;
      const col = i % columnCount;
      const row = Math.floor(i / columnCount);
      blockPositions.push({
        x: offsetX + col * (tileSize + gap),
        y: LAYOUT_PADDING_TOP + row * (cellH + gap),
        w: tileSize,
        h: cellH,
      });
      if (blockPositions.length === BLOCK_SIZE) {
        this.pushBlock(blockStart, blockPositions);
        blockPositions = [];
      }
    }

    this.pushBlock(blockStart, blockPositions);
    const rows = Math.ceil(itemCount / columnCount);
    const contentHeight = rows > 0 ? rows * cellH + (rows - 1) * gap : 0;
    this.totalHeight = contentHeight + LAYOUT_PADDING_TOP + LAYOUT_PADDING_BOTTOM;
  }

  private buildWaterfall(
    itemCount: number,
    getAspectRatio: (index: number) => number,
    colWidth: number,
    columnCount: number,
    gap: number,
    textHeight: number,
    offsetX: number,
  ): void {
    const colHeights = new Float64Array(columnCount);
    let blockStart = 0;
    let blockPositions: LayoutItem[] = [];

    for (let i = 0; i < itemCount; i += 1) {
      if (blockPositions.length === 0) blockStart = i;
      let shortest = 0;
      for (let c = 1; c < columnCount; c += 1) {
        if (colHeights[c] < colHeights[shortest]) shortest = c;
      }

      const h = colWidth / safeAspectRatio(getAspectRatio(i)) + textHeight;
      const position = {
        x: offsetX + shortest * (colWidth + gap),
        y: LAYOUT_PADDING_TOP + colHeights[shortest],
        w: colWidth,
        h,
      };
      blockPositions.push(position);
      colHeights[shortest] += h + gap;

      if (blockPositions.length === BLOCK_SIZE) {
        this.pushBlock(blockStart, blockPositions);
        blockPositions = [];
      }
    }

    this.pushBlock(blockStart, blockPositions);

    let maxHeight = 0;
    for (let c = 0; c < columnCount; c += 1) {
      if (colHeights[c] > maxHeight) maxHeight = colHeights[c];
    }
    this.totalHeight = Math.max(0, maxHeight - gap) + LAYOUT_PADDING_TOP + LAYOUT_PADDING_BOTTOM;
  }

  private buildJustified(
    itemCount: number,
    getAspectRatio: (index: number) => number,
    containerWidth: number,
    targetRowHeight: number,
    gap: number,
    textHeight: number,
    offsetX: number,
  ): void {
    let blockStart = 0;
    let blockPositions: LayoutItem[] = [];
    let y = 0;
    let rowStart = 0;

    const pushPosition = (index: number, position: LayoutItem) => {
      if (blockPositions.length === 0) blockStart = index;
      blockPositions.push(position);
      if (blockPositions.length === BLOCK_SIZE) {
        this.pushBlock(blockStart, blockPositions);
        blockPositions = [];
      }
    };

    while (rowStart < itemCount) {
      let rowEnd = rowStart;
      let totalAspect = 0;

      while (rowEnd < itemCount) {
        totalAspect += safeAspectRatio(getAspectRatio(rowEnd));
        rowEnd += 1;
        const rowWidth = totalAspect * targetRowHeight + (rowEnd - rowStart - 1) * gap;
        if (rowWidth >= containerWidth) break;
      }

      const isLastRow = rowEnd === itemCount;
      const count = rowEnd - rowStart;
      const gapSpace = (count - 1) * gap;
      const finalHeight = isLastRow
        ? targetRowHeight
        : Math.min((containerWidth - gapSpace) / totalAspect, targetRowHeight * 1.5);
      const cellH = finalHeight + textHeight;

      let x = offsetX;
      for (let i = rowStart; i < rowEnd; i += 1) {
        const w = finalHeight * safeAspectRatio(getAspectRatio(i));
        pushPosition(i, {
          x,
          y: LAYOUT_PADDING_TOP + y,
          w,
          h: cellH,
        });
        x += w + gap;
      }

      y += cellH + gap;
      rowStart = rowEnd;
    }

    this.pushBlock(blockStart, blockPositions);
    this.totalHeight = Math.max(0, y - gap) + LAYOUT_PADDING_TOP + LAYOUT_PADDING_BOTTOM;
  }
}
