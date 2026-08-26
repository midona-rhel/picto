import type { GridSpacing } from '../../shared/types/grid';

export const GRID_GAPS: Record<GridSpacing, number> = {
  wide: 16,
  tight: 8,
};

export function gridGapForSpacing(spacing: GridSpacing): number {
  return GRID_GAPS[spacing];
}
export const GRID_TILE_RADIUS = 4;
export const GRID_SELECTION_COLOR = '#3297ff';
export const GRID_SELECTION_INNER_WIDTH = 1;
export const GRID_SELECTION_OUTER_WIDTH = 2;
export const GRID_REORDER_COLOR = '#3297ff';

export const GRID_BADGE_BACKGROUND = 'rgba(0, 0, 0, 0.50)';
export const GRID_BADGE_BORDER = 'rgba(0, 0, 0, 0.20)';
export const GRID_BADGE_TEXT = 'rgba(255, 255, 255, 0.80)';
export const GRID_BADGE_INSET = 5;

// Canvas cannot resolve CSS custom properties, so keep this in the same order
// Keep canvas labels on the same platform-first stack as every DOM surface.
export const GRID_UI_FONT = '"SF Pro Text", -apple-system, "Roboto", system-ui, "Segoe UI", "Helvetica Neue", "Noto Sans", "Liberation Sans", Arial, "PingFang SC", "PingFang TC", "Hiragino Sans GB", "Microsoft Yahei", sans-serif';
export const GRID_NAME_FONT = `400 13px ${GRID_UI_FONT}`;
export const GRID_INFO_FONT = `400 11px ${GRID_UI_FONT}`;
export const GRID_BADGE_FONT = `600 10px ${GRID_UI_FONT}`;
export const GRID_RATING_FONT = `500 10px ${GRID_UI_FONT}`;

export const GRID_NAME_MARGIN_TOP = 7;
export const GRID_NAME_LINE_HEIGHT = 15;
export const GRID_INFO_MARGIN_TOP = 5;
export const GRID_INFO_LINE_HEIGHT = 15;

export const GRID_NAME_BASELINE = GRID_NAME_MARGIN_TOP + GRID_NAME_LINE_HEIGHT / 2;
export const GRID_INFO_BASELINE = GRID_INFO_MARGIN_TOP + GRID_INFO_LINE_HEIGHT / 2;
