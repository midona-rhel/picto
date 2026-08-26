import { describe, expect, it } from 'vitest';
import { appendLayout, computeStatefulLayout } from './layoutMath';

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
