/** Grid layout types — shared across layout math, canvas renderer, and visibility planning. */

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

export type GridViewMode = 'waterfall' | 'grid' | 'justified';
