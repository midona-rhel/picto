import { describe, expect, it } from 'vitest';
import {
  GRID_BADGE_BACKGROUND,
  GRID_BADGE_BORDER,
  GRID_BADGE_TEXT,
  GRID_SELECTION_COLOR,
  GRID_SELECTION_INNER_WIDTH,
  GRID_SELECTION_OUTER_WIDTH,
  GRID_TILE_RADIUS,
  gridGapForSpacing,
} from './gridAppearance';

describe('grid spacing', () => {
  it('preserves Picto wide spacing and uses reference application spacing in tight mode', () => {
    expect(gridGapForSpacing('wide')).toBe(16);
    expect(gridGapForSpacing('tight')).toBe(8);
  });

  it('uses the reference application thumbnail, selection, and badge contract', () => {
    expect(GRID_TILE_RADIUS).toBe(4);
    expect(GRID_SELECTION_COLOR).toBe('#3297ff');
    expect([GRID_SELECTION_INNER_WIDTH, GRID_SELECTION_OUTER_WIDTH]).toEqual([1, 2]);
    expect(GRID_BADGE_BACKGROUND).toBe('rgba(0, 0, 0, 0.50)');
    expect(GRID_BADGE_BORDER).toBe('rgba(0, 0, 0, 0.20)');
    expect(GRID_BADGE_TEXT).toBe('rgba(255, 255, 255, 0.80)');
  });
});
