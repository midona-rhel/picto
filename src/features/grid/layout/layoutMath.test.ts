import { describe, expect, it } from 'vitest';
import { appendLayout, computeStatefulLayout } from './layoutMath';
import { GRID_SELECTION_EXTENT } from '../gridAppearance';
import { hitTestTile } from '../canvas/hitTesting';

const CONFIG = {
  containerWidth: 500,
  targetSize: 200,
  gap: 10,
  viewMode: 'justified' as const,
  textHeight: 20,
};

function expectInsideWidth(positions: Array<{ x: number; w: number }>, width: number) {
  for (const position of positions) {
    expect(position.x).toBeGreaterThanOrEqual(0);
    expect(position.x + position.w).toBeLessThanOrEqual(width);
  }
}

describe('justified layout terminal row', () => {
  it('shrinks media rectangles to reserve a non-clickable selection gutter', () => {
    const scrollbarWidth = 12;
    const result = computeStatefulLayout(
      [1, 1, 1, 1],
      CONFIG.containerWidth,
      CONFIG.targetSize,
      CONFIG.gap,
      CONFIG.viewMode,
      CONFIG.textHeight,
      scrollbarWidth,
    );

    for (const position of result.positions) {
      expect(position.x).toBeGreaterThanOrEqual(GRID_SELECTION_EXTENT);
      expect(position.x + position.w + GRID_SELECTION_EXTENT).toBeLessThanOrEqual(CONFIG.containerWidth);
    }
    const first = result.positions[0];
    const second = result.positions[1];
    expect(second.x - (first.x + first.w)).toBeGreaterThanOrEqual(CONFIG.gap + 2 * GRID_SELECTION_EXTENT);
    expect(hitTestTile(
      result.positions,
      first.x - GRID_SELECTION_EXTENT + 1,
      first.y + (first.h - CONFIG.textHeight) / 2,
      CONFIG.textHeight,
      0,
      result.positions.length,
    )).toBeNull();
  });

  it('moves the final tile to a new row when it would overflow', () => {
    const result = computeStatefulLayout(
      [1, 1, 1],
      CONFIG.containerWidth,
      CONFIG.targetSize,
      CONFIG.gap,
      CONFIG.viewMode,
      CONFIG.textHeight,
    );

    expectInsideWidth(result.positions, CONFIG.containerWidth);
    expect(result.positions[2].y).toBeGreaterThan(result.positions[1].y);
  });

  it('preserves the same boundary when the final tile arrives by page append', () => {
    const initial = computeStatefulLayout(
      [1, 1],
      CONFIG.containerWidth,
      CONFIG.targetSize,
      CONFIG.gap,
      CONFIG.viewMode,
      CONFIG.textHeight,
    );
    const { result } = appendLayout(
      [1, 1, 1],
      2,
      initial,
      CONFIG.containerWidth,
      CONFIG.targetSize,
      CONFIG.gap,
      CONFIG.viewMode,
      CONFIG.textHeight,
    );

    expectInsideWidth(result.positions, CONFIG.containerWidth);
    expect(result.positions[2].y).toBeGreaterThan(result.positions[1].y);
  });
});
