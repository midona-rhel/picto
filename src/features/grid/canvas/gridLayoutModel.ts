import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { computeLayout, safeAspectRatio } from '../layout/layoutMath';
import { buildTileSpatialIndex, type TileSpatialIndex } from '../layout/spatialIndex';
import { adaptGridItem, type CanvasRenderItem } from './renderItemAdapter';
import type { PlanTile } from './thumbnailPlan';
import type { GridViewMode, LayoutItem } from '../layout/types';

export interface GridLayoutModel {
  items: CanvasRenderItem[];
  positions: LayoutItem[];
  spatialIndex: TileSpatialIndex;
  itemIdToIndex: Map<number, number>;
  totalHeight: number;
}

export interface ThumbnailActivationBuffers {
  activeTiles: number[];
  activeHashes: Set<string>;
  viewportHashes: Set<string>;
  planTiles: PlanTile[];
}

export function collectThumbnailActivation(
  candidates: readonly number[],
  positions: readonly LayoutItem[],
  items: readonly CanvasRenderItem[],
  activeTop: number,
  activeBottom: number,
  viewportTop: number,
  viewportBottom: number,
  buffers: ThumbnailActivationBuffers,
): void {
  buffers.activeTiles.length = 0;
  buffers.activeHashes.clear();
  buffers.viewportHashes.clear();
  let planCount = 0;
  for (const index of candidates) {
    const position = positions[index];
    const item = items[index];
    if (!position || !item || position.y + position.h < activeTop || position.y > activeBottom) continue;
    buffers.activeTiles.push(index);
    buffers.activeHashes.add(item.thumbnailHash);
    if (position.y + position.h >= viewportTop && position.y <= viewportBottom) {
      buffers.viewportHashes.add(item.hash);
    }
    const planTile = buffers.planTiles[planCount] ?? { fileHash: '', mime: '', w: 0, h: 0, cy: 0 };
    planTile.fileHash = item.thumbnailHash;
    planTile.mime = item.mime;
    planTile.w = position.w;
    planTile.h = position.h;
    planTile.cy = position.y + position.h / 2;
    buffers.planTiles[planCount] = planTile;
    planCount++;
  }
  buffers.planTiles.length = planCount;
}

/** Estimate the complete scroll extent from real loaded layout density. */
export function estimateGridScrollHeight(
  loadedHeight: number,
  loadedCount: number,
  totalCount: number | null | undefined,
): number {
  if (loadedCount === 0 || totalCount == null || totalCount <= loadedCount) return loadedHeight;
  return Math.max(loadedHeight, Math.round((loadedHeight / loadedCount) * totalCount));
}

interface LayoutConfig {
  width: number; targetSize: number; gap: number; viewMode: GridViewMode;
  textHeight: number; scrollbarWidth: number;
}

function createModel(
  source: CanonicalEntityGridItem[],
  config: LayoutConfig,
  prefix?: { items: CanvasRenderItem[]; ratios: number[]; itemIdToIndex: Map<number, number> },
): { model: GridLayoutModel; ratios: number[] } {
  const start = prefix?.items.length ?? 0;
  const items = prefix ? [...prefix.items] : new Array<CanvasRenderItem>(source.length);
  const ratios = prefix ? [...prefix.ratios] : new Array<number>(source.length);
  const itemIdToIndex = prefix?.itemIdToIndex ?? new Map<number, number>();
  for (let i = start; i < source.length; i++) {
    const item = adaptGridItem(source[i]);
    items[i] = item;
    ratios[i] = safeAspectRatio(item.aspectRatio ?? 1.5);
    itemIdToIndex.set(item.itemId, i);
  }
  const layout = computeLayout(
    ratios,
    config.width,
    config.targetSize,
    config.gap,
    config.viewMode,
    config.textHeight,
    config.scrollbarWidth,
  );
  return { ratios, model: {
    items,
    positions: layout.positions,
    spatialIndex: buildTileSpatialIndex(layout.positions),
    itemIdToIndex,
    totalHeight: layout.totalHeight,
  } };
}

/** Retains immutable item adaptation across page appends; every other change rebuilds. */
export class GridLayoutRuntime {
  private source: CanonicalEntityGridItem[] = [];
  private config: LayoutConfig | null = null;
  private model: GridLayoutModel | null = null;
  private ratios: number[] = [];

  update(source: CanonicalEntityGridItem[], config: LayoutConfig): GridLayoutModel {
    if (this.source === source && this.config && sameConfig(this.config, config)) return this.model!;
    const canAppend = this.model != null && this.config != null && sameConfig(this.config, config)
      && source.length > this.source.length
      && this.source.every((item, index) => source[index] === item);
    const result = createModel(source, config, canAppend ? {
      items: this.model!.items,
      ratios: this.ratios,
      itemIdToIndex: this.model!.itemIdToIndex,
    } : undefined);
    this.source = source;
    this.config = config;
    this.model = result.model;
    this.ratios = result.ratios;
    return result.model;
  }
}

function sameConfig(a: LayoutConfig, b: LayoutConfig) {
  return a.width === b.width && a.targetSize === b.targetSize && a.gap === b.gap
    && a.viewMode === b.viewMode && a.textHeight === b.textHeight
    && a.scrollbarWidth === b.scrollbarWidth;
}
