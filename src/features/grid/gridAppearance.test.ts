import { describe, expect, it } from 'vitest';
import {
  GRID_BADGE_BACKGROUND,
  GRID_BADGE_BORDER,
  GRID_BADGE_FONT,
  GRID_BADGE_INSET,
  GRID_BADGE_TEXT,
  GRID_INFO_BASELINE,
  GRID_INFO_FONT,
  GRID_NAME_BASELINE,
  GRID_NAME_FONT,
  GRID_SELECTION_COLOR,
  GRID_SELECTION_INNER_WIDTH,
  GRID_SELECTION_OUTER_WIDTH,
  GRID_TILE_RADIUS,
  GRID_UI_FONT,
  gridGapForSpacing,
} from './gridAppearance';
import { uiFontStack } from '../../shared/lib/platform';

describe('grid spacing', () => {
  it('preserves wide spacing and uses compact spacing in tight mode', () => {
    expect(gridGapForSpacing('wide')).toBe(16);
    expect(gridGapForSpacing('tight')).toBe(8);
  });

  it('uses the canonical thumbnail, selection, and badge contract', () => {
    expect(GRID_TILE_RADIUS).toBe(4);
    expect(GRID_SELECTION_COLOR).toBe('#3297ff');
    expect([GRID_SELECTION_INNER_WIDTH, GRID_SELECTION_OUTER_WIDTH]).toEqual([1, 2]);
    expect(GRID_BADGE_BACKGROUND).toBe('rgba(0, 0, 0, 0.50)');
    expect(GRID_BADGE_BORDER).toBe('rgba(0, 0, 0, 0.20)');
    expect(GRID_BADGE_TEXT).toBe('rgba(255, 255, 255, 0.80)');
    expect(GRID_BADGE_INSET).toBe(5);
  });

  it('uses canonical grid typography and vertical metrics', () => {
    expect(GRID_UI_FONT).toBe(uiFontStack());
    expect(GRID_NAME_FONT).toBe(`400 13px ${GRID_UI_FONT}`);
    expect(GRID_INFO_FONT).toBe(`400 11px ${GRID_UI_FONT}`);
    expect(GRID_BADGE_FONT).toBe(`600 10px ${GRID_UI_FONT}`);
    expect(GRID_NAME_BASELINE).toBe(14.5);
    expect(GRID_INFO_BASELINE).toBe(12.5);
  });
});
