import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { appendLayout, computeStatefulLayout, safeAspectRatio, type StatefulLayoutResult } from '../layout/layoutMath';
import { buildTileSpatialIndex, type TileSpatialIndex } from '../layout/spatialIndex';
import { adaptGridItem, type CanvasRenderItem } from './renderItemAdapter';
import type { GridViewMode, LayoutItem } from '../layout/types';

export interface GridLayoutModel {
  items: CanvasRenderItem[];
  positions: LayoutItem[];
  spatialIndex: TileSpatialIndex;
  hashToIndex: Map<string, number>;
  totalHeight: number;
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
  prefix?: {
    items: CanvasRenderItem[];
    ratios: number[];
    hashToIndex: Map<string, number>;
    layout: StatefulLayoutResult;
  },
): { model: GridLayoutModel; ratios: number[]; layout: StatefulLayoutResult } {
  const start = prefix?.items.length ?? 0;
  const items = prefix?.items ?? new Array<CanvasRenderItem>(source.length);
  const ratios = prefix?.ratios ?? new Array<number>(source.length);
  items.length = ratios.length = source.length;
  const hashToIndex = prefix?.hashToIndex ?? new Map<string, number>();
  for (let i = start; i < source.length; i++) {
    const item = adaptGridItem(source[i]);
    items[i] = item;
    ratios[i] = safeAspectRatio(item.aspectRatio ?? 1.5);
    hashToIndex.set(item.hash, i);
  }
  const layout = prefix
    ? appendLayout(
      ratios, start, prefix.layout, config.width, config.targetSize, config.gap,
      config.viewMode, config.textHeight, config.scrollbarWidth,
    ).result
    : computeStatefulLayout(
      ratios, config.width, config.targetSize, config.gap,
      config.viewMode, config.textHeight, config.scrollbarWidth,
    );
  return { ratios, model: {
    items,
    positions: layout.positions,
    spatialIndex: buildTileSpatialIndex(layout.positions),
    hashToIndex,
    totalHeight: layout.totalHeight,
  }, layout };
}

/** Retains immutable item adaptation across page appends; every other change rebuilds. */
export class GridLayoutRuntime {
  private source: CanonicalEntityGridItem[] = [];
  private config: LayoutConfig | null = null;
  private model: GridLayoutModel | null = null;
  private ratios: number[] = [];
  private layout: StatefulLayoutResult | null = null;

  update(source: CanonicalEntityGridItem[], config: LayoutConfig): GridLayoutModel {
    if (this.source === source && this.config && sameConfig(this.config, config)) return this.model!;
    const canAppend = this.model != null && this.config != null && sameConfig(this.config, config)
      && source.length > this.source.length
      && this.source.every((item, index) => source[index] === item);
    const result = createModel(source, config, canAppend ? {
      items: this.model!.items,
      ratios: this.ratios,
      hashToIndex: this.model!.hashToIndex,
      layout: this.layout!,
    } : undefined);
    this.source = source;
    this.config = config;
    this.model = result.model;
    this.ratios = result.ratios;
    this.layout = result.layout;
    return result.model;
  }
}

function sameConfig(a: LayoutConfig, b: LayoutConfig) {
  return a.width === b.width && a.targetSize === b.targetSize && a.gap === b.gap
    && a.viewMode === b.viewMode && a.textHeight === b.textHeight
    && a.scrollbarWidth === b.scrollbarWidth;
}
