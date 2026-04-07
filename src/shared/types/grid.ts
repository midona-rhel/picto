export type GridViewMode = 'waterfall' | 'grid' | 'justified';

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
